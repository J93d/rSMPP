use slint::{Model, SharedString, VecModel, Weak};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

mod bind_transmitter;
mod smpp_error_codes;
mod submit_sm;

use bind_transmitter::{BindTransmitter, BindTransmitterBuilder};
use submit_sm::{SubmitSm, SubmitSmBuilder, TypeOfNumber, NumericPlanIndicator};

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = AppWindow::new()?;
    let main_window_weak = main_window.as_weak();

    let (tx_log, mut rx_log) = mpsc::channel::<String>(100);
    let (tx_cmd, mut rx_cmd) = mpsc::channel::<Cmd>(100);

    // Create a runtime for async tasks
    let rt = tokio::runtime::Runtime::new()?;

    // Logging Task
    let log_window_weak = main_window_weak.clone();
    rt.spawn(async move {
        while let Some(log) = rx_log.recv().await {
            let log_window_weak = log_window_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = log_window_weak.upgrade() {
                    let logs = window.get_logs();
                    let mut vec: Vec<SharedString> = logs.iter().collect();
                    vec.push(SharedString::from(log));
                    let model = Rc::new(VecModel::from(vec));
                    window.set_logs(model.into());
                }
            });
        }
    });

    // SMPP Client Task
    rt.spawn(async move {
        let mut stream: Option<TcpStream> = None;
        let tx_log = tx_log.clone();

        while let Some(cmd) = rx_cmd.recv().await {
            match cmd {
                Cmd::Connect { ip, port, system_id, password } => {
                    let addr = format!("{}:{}", ip, port);
                    match TcpStream::connect(&addr).await {
                        Ok(s) => {
                            stream = Some(s);
                            let _ = tx_log.send(format!("Connected to {}", addr)).await;
                            
                            // Send Bind Transmitter
                            let bind_builder = BindTransmitterBuilder::new(system_id, password);
                            match BindTransmitter::bind_transmitter_async(bind_builder).await {
                                Ok(pdu) => {
                                    if let Some(ref mut s) = stream {
                                        if let Err(e) = s.write_all(&pdu).await {
                                            let _ = tx_log.send(format!("Failed to send bind PDU: {}", e)).await;
                                        } else {
                                            let _ = tx_log.send("Sent Bind Transmitter PDU".to_string()).await;
                                            
                                            // Read response
                                            let mut buffer = [0u8; 1024];
                                            match s.read(&mut buffer).await {
                                                Ok(n) if n > 0 => {
                                                    match BindTransmitter::parse_bind_transmitter_resp(&buffer[..n]).await {
                                                        Ok(resp) => {
                                                            let _ = tx_log.send(format!("Bind Response: {} ({})", resp.status_name, resp.command_status)).await;
                                                        }
                                                        Err(e) => {
                                                            let _ = tx_log.send(format!("Failed to parse bind response: {}", e)).await;
                                                        }
                                                    }
                                                }
                                                Ok(_) => {
                                                     let _ = tx_log.send("Connection closed by server during bind".to_string()).await;
                                                }
                                                Err(e) => {
                                                    let _ = tx_log.send(format!("Failed to read bind response: {}", e)).await;
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = tx_log.send(format!("Failed to create bind PDU: {}", e)).await;
                                }
                            }

                        }
                        Err(e) => {
                            let _ = tx_log.send(format!("Failed to connect to {}: {}", addr, e)).await;
                        }
                    }
                }
                Cmd::Disconnect => {
                     stream = None;
                     let _ = tx_log.send("Disconnected".to_string()).await;
                }
                Cmd::SendMessage { source, dest, message } => {
                     if let Some(ref mut s) = stream {
                        let mut builder = SubmitSmBuilder::new();
                        builder = builder.source(TypeOfNumber::Unknown, NumericPlanIndicator::Unknown, source);
                        builder = builder.destination(TypeOfNumber::Unknown, NumericPlanIndicator::Unknown, dest);
                        builder = builder.message(message);
                        
                        match SubmitSm::create_pdu(builder.build()).await {
                            Ok(pdu) => {
                                if let Err(e) = s.write_all(&pdu).await {
                                    let _ = tx_log.send(format!("Failed to send submit_sm: {}", e)).await;
                                } else {
                                    let _ = tx_log.send("Sent Submit SM PDU".to_string()).await;
                                    
                                     // Read response - Simple implementation, assumes synchronous response for demo
                                     let mut buffer = [0u8; 1024];
                                    match s.read(&mut buffer).await {
                                        Ok(n) if n > 0 => {
                                            match SubmitSm::parse_submit_sm_resp(&buffer[..n]).await {
                                                Ok(resp) => {
                                                     let _ = tx_log.send(format!("Submit SM Response: {} (Msg ID: {:?})", resp.status_name, resp.message_id)).await;
                                                }
                                                Err(e) => {
                                                    let _ = tx_log.send(format!("Failed to parse submit response: {}", e)).await;
                                                }
                                            }
                                        }
                                         Ok(_) => {
                                              let _ = tx_log.send("Connection closed by server".to_string()).await;
                                         }
                                        Err(e) => {
                                             let _ = tx_log.send(format!("Failed to read submit response: {}", e)).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx_log.send(format!("Failed to create submit_sm PDU: {}", e)).await;
                            }
                        }
                     } else {
                         let _ = tx_log.send("Not connected".to_string()).await;
                     }
                }
            }
        }
    });

    // UI Callbacks
    let tx_cmd_connect = tx_cmd.clone();
    main_window.on_connect(move |ip, port, sys_id, pass| {
        let _ = tx_cmd_connect.blocking_send(Cmd::Connect {
            ip: ip.into(),
            port: port.into(),
            system_id: sys_id.into(),
            password: pass.into(),
        });
        
        // Optimistic UI update - real app should wait for result
    });

    let tx_cmd_send = tx_cmd.clone();
    main_window.on_send_message(move |src, dest, msg| {
         let _ = tx_cmd_send.blocking_send(Cmd::SendMessage {
            source: src.into(),
            dest: dest.into(),
            message: msg.into(),
        });
    });

    main_window.run()?;
    Ok(())
}

enum Cmd {
    Connect { ip: String, port: String, system_id: String, password: String },
    Disconnect,
    SendMessage { source: String, dest: String, message: String },
}
