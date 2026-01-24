mod common;
mod bind;
mod gsm_encoding;
mod submit_sm;
mod enquire_link;
mod deliver_sm;

use common::command_id;
use bind::{Bind, BindBuilder, BindMode};
use submit_sm::{SubmitSm, Encoding, MultipartMode}; // NumericPlanIndicator, TypeOfNumber removed
use enquire_link::EnquireLink;

use slint::ComponentHandle;
use slint::{Model, SharedString, VecModel}; 
use std::rc::Rc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tokio::select;

slint::include_modules!();

enum UiEvent {
    Log(String),
    ConnectionStatus(String, bool), // Status text, is_connected
}

enum Cmd {
    Connect { ip: String, port: String, system_id: String, password: String, bind_mode: String },
    Disconnect,
    SendMessage { source: String, dest: String, message: String, encoding: String, mode: String },
}

enum WriterCmd {
    Write(Vec<u8>),
    Close,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = AppWindow::new()?;
    let main_window_weak = main_window.as_weak();

    let (tx_ui, mut rx_ui) = mpsc::channel::<UiEvent>(100);
    let (tx_cmd, mut rx_cmd) = mpsc::channel::<Cmd>(100);

    // Create a runtime for async tasks
    let rt = tokio::runtime::Runtime::new()?;

    // UI Updater Task
    let ui_window_weak = main_window_weak.clone();
    rt.spawn(async move {
        while let Some(event) = rx_ui.recv().await {
            let ui_window_weak = ui_window_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = ui_window_weak.upgrade() {
                    match event {
                        UiEvent::Log(log) => {
                            let logs = window.get_logs();
                            let mut vec: Vec<SharedString> = logs.iter().collect();
                            vec.push(SharedString::from(log));
                            let model = Rc::new(VecModel::from(vec));
                            window.set_logs(model.into());
                        }
                        UiEvent::ConnectionStatus(status, connected) => {
                            window.set_connection_status(SharedString::from(status));
                            window.set_is_connected(connected);
                        }
                    }
                }
            });
        }
    });

    // Main Logic Task
    rt.spawn(async move {
        let mut tx_writer: Option<mpsc::Sender<WriterCmd>> = None;
        let mut connection_handle: Option<tokio::task::JoinHandle<()>> = None;

        while let Some(cmd) = rx_cmd.recv().await {
            match cmd {
                Cmd::Connect { ip, port, system_id, password, bind_mode } => {
                    let addr = format!("{}:{}", ip, port);
                    match TcpStream::connect(&addr).await {
                        Ok(stream) => {
                            let _ = tx_ui.send(UiEvent::Log(format!("Connected to {}", addr))).await;
                             let _ = tx_ui.send(UiEvent::ConnectionStatus("Connected".to_string(), true)).await;
                            
                            let (mut reader, mut writer) = stream.into_split();
                            let (tx_w, mut rx_w) = mpsc::channel::<WriterCmd>(100);
                            tx_writer = Some(tx_w.clone());

                            let tx_ui_clone = tx_ui.clone();
                            let connection_task = tokio::spawn(async move {
                                let mut interval = time::interval(Duration::from_secs(5));
                                // First tick returns immediately, so skip or use wait
                                
                                loop {
                                    select! {
                                        // Writer Loop
                                        Some(w_cmd) = rx_w.recv() => {
                                            match w_cmd {
                                                WriterCmd::Write(data) => {
                                                    if let Err(e) = writer.write_all(&data).await {
                                                        let _ = tx_ui_clone.send(UiEvent::Log(format!("Write Error: {}", e))).await;
                                                        let _ = tx_ui_clone.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                        break;
                                                    }
                                                }
                                                WriterCmd::Close => break,
                                            }
                                        }
                                        
                                        // Heartbeat Loop
                                        _ = interval.tick() => {
                                            // Send EnquireLink
                                            let pdu = EnquireLink::create_pdu();
                                             if let Err(e) = writer.write_all(&pdu).await {
                                                 let _ = tx_ui_clone.send(UiEvent::Log(format!("Heartbeat Error: {}", e))).await;
                                                 let _ = tx_ui_clone.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                 break;
                                             }
                                             // let _ = tx_ui_clone.send(UiEvent::Log("Sent EnquireLink".to_string())).await; 
                                             // Commented out to avoid spamming logs, enable if needed for debug
                                        }

                                        // Reader Loop (Simple Check)
                                        // Since we can't easily select! on read_exact without cancellation safety issues or buf management...
                                        // ...actually, TcpStream splitting allows concurrent read/write tasks.
                                        // But here we put them in one select! loop? No, reader needs its own task or future.
                                        // Let's spawn reader separately to avoid select! issues with reading.
                                    }
                                }
                            });
                            
                            // Spawn Reader separately
                            let tx_ui_read = tx_ui.clone();
                            tokio::spawn(async move {
                                let mut buffer = vec![0u8; 1024];
                                loop {
                                    // Read Header Length (4 bytes)
                                    let mut len_buf = [0u8; 4];
                                    match reader.read_exact(&mut len_buf).await {
                                        Ok(_) => {
                                            let len = u32::from_be_bytes(len_buf) as usize;
                                            if len < 4 || len > 1024 * 64 { // Sanity check
                                                let _ = tx_ui_read.send(UiEvent::Log(format!("Invalid PDU length: {}", len))).await;
                                                break;
                                            }
                                            
                                            // Read rest of PDU
                                            if buffer.len() < len {
                                                buffer.resize(len, 0);
                                            }
                                            // Copy len back
                                            buffer[0..4].copy_from_slice(&len_buf);
                                            
                                            match reader.read_exact(&mut buffer[4..len]).await {
                                                Ok(_) => {
                                                    // Parse Command ID
                                                    let cmd_id = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
                                                    match cmd_id {
                                                        command_id::BIND_RECEIVER_RESP | 
                                                        command_id::BIND_TRANSMITTER_RESP | 
                                                        command_id::BIND_TRANSCEIVER_RESP => {
                                                            match Bind::parse_bind_resp(&buffer[..len]).await {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Bind Resp: {} ({})", resp.status_name, resp.command_status))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {}", e))).await; }
                                                            }
                                                        },
                                                        command_id::SUBMIT_SM_RESP => {
                                                            match SubmitSm::parse_submit_sm_resp(&buffer[..len]).await {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Submit Resp: {} (Msg ID: {:?})", resp.status_name, resp.message_id))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {}", e))).await; }
                                                            }
                                                        },
                                                        command_id::DELIVER_SM => {
                                                            match deliver_sm::deliver_sm_async(&buffer[..len]).await {
                                                                Ok(result) => {
                                                                     let _ = tx_ui_read.send(UiEvent::Log(format!("Recv DeliverSM: {:?} from {:?}", result.message, result.orig_addr))).await;
                                                                }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("DeliverSM Parse Error: {}", e))).await; }
                                                            }
                                                        },
                                                        command_id::ENQUIRE_LINK_RESP => {
                                                             // let _ = tx_ui_read.send(UiEvent::Log("Recv EnquireLinkResp".to_string())).await;
                                                        },
                                                        _ => {
                                                            let _ = tx_ui_read.send(UiEvent::Log(format!("Recv PDU: CmdID 0x{:08X}", cmd_id))).await;
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = tx_ui_read.send(UiEvent::Log(format!("Read Error Body: {}", e))).await;
                                                     let _ = tx_ui_read.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                    break;
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            // Connection closed
                                             let _ = tx_ui_read.send(UiEvent::Log("Connection closed by peer".to_string())).await;
                                             let _ = tx_ui_read.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                            break;
                                        }
                                    }
                                }
                            });
                            
                            connection_handle = Some(connection_task);

                            // Send Bind
                            let mode_enum = match bind_mode.as_str() {
                                "Transmitter" => BindMode::Transmitter,
                                "Receiver" => BindMode::Receiver,
                                "Transceiver" => BindMode::Transceiver,
                                _ => BindMode::Transceiver,
                            };
                            let bind_builder = BindBuilder::new(mode_enum, system_id, password);
                            if let Ok(pdu) = Bind::bind_async(bind_builder).await {
                                let _ = tx_writer.as_ref().unwrap().send(WriterCmd::Write(pdu)).await;
                            }
                        }
                        Err(e) => {
                            let _ = tx_ui.send(UiEvent::Log(format!("Failed to connect: {}", e))).await;
                            let _ = tx_ui.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                        }
                    }
                }
                Cmd::Disconnect => {
                    if let Some(mut tx) = tx_writer.take() {
                        let _ = tx.send(WriterCmd::Close).await;
                    }
                    if let Some(handle) = connection_handle.take() {
                        handle.abort();
                    }
                     let _ = tx_ui.send(UiEvent::Log("Disconnected".to_string())).await;
                     let _ = tx_ui.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                }
                Cmd::SendMessage { source, dest, message, encoding, mode } => {
                     if let Some(tx) = &tx_writer {
                        let enc_enum = match encoding.as_str() {
                            "GSM 7-bit" => Encoding::Gsm7Bit,
                            "Latin-1 (8-bit)" => Encoding::Latin1,
                            "UCS-2 (16-bit)" => Encoding::Ucs2,
                            _ => Encoding::Gsm7Bit,
                        };

                        let mode_enum = match mode.as_str() {
                            "UDH" => MultipartMode::Udh,
                            "SAR" => MultipartMode::Sar,
                            "Payload" => MultipartMode::Payload,
                            _ => MultipartMode::Udh,
                        };

                        match SubmitSm::create_pdus(source, dest, message, enc_enum, mode_enum).await {
                            Ok(pdus) => {
                                let total = pdus.len();
                                for (i, pdu) in pdus.iter().enumerate() {
                                     let _ = tx.send(WriterCmd::Write(pdu.to_vec())).await;
                                     let _ = tx_ui.send(UiEvent::Log(format!("Sent Segment {}/{}", i+1, total))).await;
                                }
                            }
                            Err(e) => {
                                let _ = tx_ui.send(UiEvent::Log(format!("Error creating PDUs: {}", e))).await;
                            }
                        }
                     } else {
                         let _ = tx_ui.send(UiEvent::Log("Not connected".to_string())).await;
                     }
                }
            }
        }
    });

    // UI Callbacks
    let tx_cmd_connect = tx_cmd.clone();
    main_window.on_connect(move |ip, port, sys_id, pass, bind_mode| {
        let _ = tx_cmd_connect.blocking_send(Cmd::Connect {
            ip: ip.into(),
            port: port.into(),
            system_id: sys_id.into(),
            password: pass.into(),
            bind_mode: bind_mode.into(),
        });
    });

    let tx_cmd_send = tx_cmd.clone();
    main_window.on_send_message(move |src, dest, msg, enc, mode| {
         let _ = tx_cmd_send.blocking_send(Cmd::SendMessage {
            source: src.into(),
            dest: dest.into(),
            message: msg.into(),
            encoding: enc.into(),
            mode: mode.into(),
        });
    });

    main_window.run()?;
    Ok(())
}
