#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod common;
mod bind;
mod gsm_encoding;
mod submit_sm;
mod enquire_link;
mod deliver_sm;

use common::command_id;
use bind::{Bind, BindBuilder, BindMode};
use submit_sm::{SubmitSm, Encoding, MultipartMode};
use enquire_link::EnquireLink;

use slint::ComponentHandle;
use slint::{Model, SharedString, VecModel}; 
use std::rc::Rc;
use std::sync::Arc; // For Arc
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tokio::select;
use rand::Rng;

// TLS Imports
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::client::danger::{ServerCertVerified, ServerCertVerifier, HandshakeSignatureValid};
use tokio_rustls::rustls::DigitallySignedStruct;
use tokio_rustls::rustls::SignatureScheme;

slint::include_modules!();

enum UiEvent {
    Log(String),
    ConnectionStatus(String, bool), // Status text, is_connected
}

enum Cmd {
    Connect { ip: String, port: String, system_id: String, password: String, bind_mode: String, use_ssl: bool },
    Unbind,
    SendMessage { 
        source: String, src_ton: String, src_npi: String,
        dest: String, dest_ton: String, dest_npi: String,
        message: String, encoding: String, mode: String,
        pid: String, dcs: String, validity: String, dlr: bool 
    },
}

enum WriterCmd {
    Write(Vec<u8>),
}

// Dangerous Verifier to skip certificate validation
#[derive(Debug)]
struct DangerousVerifier;

impl ServerCertVerifier for DangerousVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // Accept Any Certificate
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        tokio_rustls::rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
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

        while let Some(cmd) = rx_cmd.recv().await {
            match cmd {
                Cmd::Connect { ip, port, system_id, password, bind_mode, use_ssl } => {
                    let addr = format!("{}:{}", ip, port);
                    
                    // --- CONNECTION LOGIC ---
                    let result: Result<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>), Box<dyn std::error::Error + Send + Sync>> = async {
                        let tcp_stream = TcpStream::connect(&addr).await?;
                        
                        if use_ssl {
                            // Setup TLS with Dangerous Verifier
                            let root_store = RootCertStore::empty();
                            let mut config = ClientConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
                                .with_protocol_versions(&[&tokio_rustls::rustls::version::TLS12, &tokio_rustls::rustls::version::TLS13])?
                                .with_root_certificates(root_store)
                                .with_no_client_auth();
                            
                            config.dangerous().set_certificate_verifier(Arc::new(DangerousVerifier));
                            
                            let connector = TlsConnector::from(Arc::new(config));
                            
                            // Use IP as ServerName or fallback
                            let domain = ServerName::try_from(ip.as_str())
                                .or_else(|_| ServerName::try_from("example.com"))?;
                                
                            let tls_stream = connector.connect(domain.to_owned(), tcp_stream).await?;
                            let (r, w) = tokio::io::split(tls_stream);
                            Ok((Box::new(r) as Box<dyn AsyncRead + Unpin + Send>, Box::new(w) as Box<dyn AsyncWrite + Unpin + Send>))
                        } else {
                            let (r, w) = tcp_stream.into_split();
                            Ok((Box::new(r) as Box<dyn AsyncRead + Unpin + Send>, Box::new(w) as Box<dyn AsyncWrite + Unpin + Send>))
                        }
                    }.await;

                    match result {
                        Ok((mut reader, mut writer)) => {
                            let _ = tx_ui.send(UiEvent::Log(format!("Connected to {} (SSL: {})", addr, use_ssl))).await;
                             let _ = tx_ui.send(UiEvent::ConnectionStatus("Connected".to_string(), true)).await;
                            
                            let (tx_w, mut rx_w) = mpsc::channel::<WriterCmd>(100);
                            tx_writer = Some(tx_w.clone());

                            let tx_ui_clone = tx_ui.clone();
                            let _ = tokio::spawn(async move {
                                let mut interval = time::interval(Duration::from_secs(5));
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
                                            }
                                        }
                                        
                                        // Heartbeat Loop
                                        _ = interval.tick() => {
                                            let pdu = EnquireLink::create_pdu();
                                             if let Err(e) = writer.write_all(&pdu).await {
                                                 let _ = tx_ui_clone.send(UiEvent::Log(format!("Heartbeat Error: {}", e))).await;
                                                 let _ = tx_ui_clone.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                 break;
                                             }
                                        }
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
                                                                    let mut log_msg = format!("DeliverSM From: {} To: {}", 
                                                                        result.orig_addr.unwrap_or_default(), 
                                                                        result.dest_addr.unwrap_or_default()
                                                                    );
                                                                    
                                                                    if let Some(msg) = result.message {
                                                                        log_msg.push_str(&format!(" Msg: \"{}\"", msg));
                                                                    }
                                                                    
                                                                    if let (Some(id), Some(stat)) = (result.msg_id, result.msg_status) {
                                                                        log_msg.push_str(&format!(" [DLR: ID={} Stat={}]", id, stat));
                                                                    }
                                                                    
                                                                     let _ = tx_ui_read.send(UiEvent::Log(log_msg)).await;
                                                                }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("DeliverSM Parse Error: {}", e))).await; }
                                                            }
                                                        },
                                                        0x80000006 => { // UNBIND_RESP
                                                            let _ = tx_ui_read.send(UiEvent::Log("Unbind Response Received".to_string())).await;
                                                            let _ = tx_ui_read.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                        },
                                                        command_id::ENQUIRE_LINK_RESP => {
                                                             // Only log if verbose
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
                            
                            // Connection task spawned above

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
                Cmd::Unbind => {
                     if let Some(tx) = &tx_writer {
                        let _ = tx_ui.send(UiEvent::Log("Sending Unbind...".to_string())).await;
                        // Create and send Unbind PDU (0x00000006)
                        let mut pdu = Vec::new();
                        pdu.extend_from_slice(&16u32.to_be_bytes()); // Length
                        pdu.extend_from_slice(&0x00000006u32.to_be_bytes()); // UNBIND
                        pdu.extend_from_slice(&0u32.to_be_bytes()); // Status
                        pdu.extend_from_slice(&rand::thread_rng().gen_range(1..10000u32).to_be_bytes()); // Seq
                        
                        let _ = tx.send(WriterCmd::Write(pdu)).await;
                     }
                }
                Cmd::SendMessage { source, src_ton, src_npi, dest, dest_ton, dest_npi, message, encoding, mode, pid, dcs, validity, dlr } => {
                     if let Some(tx) = &tx_writer {
                        let enc_enum = match encoding.as_str() {
                            "GSM 7-bit" => Encoding::Gsm7Bit,
                            "Latin-1" => Encoding::Latin1,
                            "UCS-2" => Encoding::Ucs2,
                            _ => Encoding::Gsm7Bit,
                        };

                        let mode_enum = match mode.as_str() {
                            "UDH" => MultipartMode::Udh,
                            "SAR" => MultipartMode::Sar,
                            "Payload" => MultipartMode::Payload,
                            _ => MultipartMode::Udh,
                        };

                        match SubmitSm::create_pdus(
                            source, src_ton.parse().unwrap_or(1), src_npi.parse().unwrap_or(1),
                            dest, dest_ton.parse().unwrap_or(1), dest_npi.parse().unwrap_or(1),
                            message, enc_enum, mode_enum,
                            pid.parse().unwrap_or(0), dcs.parse().ok(), validity, dlr
                        ).await {
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
    main_window.on_connect(move |ip, port, sys_id, pass, bind_mode, use_ssl| {
        let _ = tx_cmd_connect.blocking_send(Cmd::Connect {
            ip: ip.into(),
            port: port.into(),
            system_id: sys_id.into(),
            password: pass.into(),
            bind_mode: bind_mode.into(),
            use_ssl // Pass boolean
        });
    });

    let tx_cmd_unbind = tx_cmd.clone();
    main_window.on_unbind(move || {
        let _ = tx_cmd_unbind.blocking_send(Cmd::Unbind);
    });

    let tx_cmd_send = tx_cmd.clone();
    main_window.on_send_message(move |src, src_ton, src_npi, dest, dest_ton, dest_npi, msg, enc, mode, pid, dcs, val, dlr| {
         let _ = tx_cmd_send.blocking_send(Cmd::SendMessage {
            source: src.into(),
            src_ton: src_ton.into(),
            src_npi: src_npi.into(),
            dest: dest.into(),
            dest_ton: dest_ton.into(),
            dest_npi: dest_npi.into(),
            message: msg.into(),
            encoding: enc.into(),
            mode: mode.into(),
            pid: pid.into(),
            dcs: dcs.into(),
            validity: val.into(),
            dlr
        });
    });

    main_window.run()?;
    Ok(())
}
