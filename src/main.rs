#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rand::Rng;
use slint::ComponentHandle;
use slint::{Model, SharedString, VecModel};
use std::rc::Rc;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

// TLS Imports
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::DigitallySignedStruct;
use tokio_rustls::rustls::SignatureScheme;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{self, ClientConfig, RootCertStore};

// SMPP Codec Imports
use smpp_codec::common::{
    BindMode, CMD_BIND_RECEIVER_RESP, CMD_BIND_TRANSCEIVER_RESP, CMD_BIND_TRANSMITTER_RESP,
    CMD_DELIVER_SM, CMD_ENQUIRE_LINK, CMD_ENQUIRE_LINK_RESP, CMD_SUBMIT_MULTI_SM_RESP,
    CMD_SUBMIT_SM_RESP, CMD_UNBIND_RESP, Npi, Ton,
};
use smpp_codec::common::{CMD_CANCEL_SM_RESP, CMD_QUERY_SM_RESP, CMD_REPLACE_SM_RESP};
use smpp_codec::pdus::{
    BindRequest, BindResponse, CancelSmRequest, CancelSmResponse, DeliverSmRequest,
    DeliverSmResponse, Destination, EnquireLinkRequest, QuerySmRequest, QuerySmResponse, ReplaceSm,
    ReplaceSmResp, SubmitMulti, SubmitMultiResp, SubmitSmRequest, SubmitSmResponse, UnbindRequest,
};
use smpp_codec::splitter::{EncodingType, MessageSplitter, SplitMode};
use smpp_codec::tlv::{Tlv, tags};

slint::include_modules!();

enum UiEvent {
    Log(String),
    ConnectionStatus(String, bool),
}

enum Cmd {
    Connect {
        ip: String,
        port: String,
        system_id: String,
        password: String,
        bind_mode: String,
        use_ssl: bool,
    },
    Unbind,
    SendMessage {
        source: String,
        src_ton: String,
        src_npi: String,
        dest: String,
        dest_ton: String,
        dest_npi: String,
        message: String,
        encoding: String,
        mode: String,
        pid: String,
        dcs: String,
        validity: String,
        dlr: bool,
    },
    QuerySm {
        msg_id: String,
        source: String,
        ton: String,
        npi: String,
    },
    CancelSm {
        msg_id: String,
        source: String,
        src_ton: String,
        src_npi: String,
        dest: String,
        dest_ton: String,
        dest_npi: String,
    },
    ReplaceSm {
        msg_id: String,
        source: String,
        src_ton: String,
        src_npi: String,
        message: String,
    },
}

enum WriterCmd {
    Write(Vec<u8>),
    Close,
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
        tokio_rustls::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[allow(clippy::type_complexity)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let main_window = AppWindow::new()?;
    let main_window_weak = main_window.as_weak();

    let (tx_ui, mut rx_ui) = mpsc::channel::<UiEvent>(100);
    let (tx_cmd, mut rx_cmd) = mpsc::channel::<Cmd>(100);

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

                    let result: Result<(Box<dyn AsyncRead + Unpin + Send>, Box<dyn AsyncWrite + Unpin + Send>), Box<dyn std::error::Error + Send + Sync>> = async {
                        let tcp_stream = TcpStream::connect(&addr).await?;

                        if use_ssl {
                            let root_store = RootCertStore::empty();
                            let mut config = ClientConfig::builder_with_provider(Arc::new(tokio_rustls::rustls::crypto::ring::default_provider()))
                                .with_protocol_versions(&[&tokio_rustls::rustls::version::TLS12, &tokio_rustls::rustls::version::TLS13])?
                                .with_root_certificates(root_store)
                                .with_no_client_auth();

                            config.dangerous().set_certificate_verifier(Arc::new(DangerousVerifier));

                            let connector = TlsConnector::from(Arc::new(config));

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
                            tokio::spawn(async move {
                                let mut interval = time::interval(Duration::from_secs(5));
                                loop {
                                    select! {
                                        cmd = rx_w.recv() => {
                                            match cmd {
                                                Some(w_cmd) => {
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
                                                None => break,
                                            }
                                        }
                                        // Heartbeat Loop
                                        _ = interval.tick() => {
                                            let mut pdu = Vec::new();
                                            let req = EnquireLinkRequest::new(rand::thread_rng().gen_range(1..10000));
                                            #[allow(clippy::collapsible_if)]
                                            if req.encode(&mut pdu).is_ok() {
                                                if let Err(e) = writer.write_all(&pdu).await {
                                                     let _ = tx_ui_clone.send(UiEvent::Log(format!("Heartbeat Error: {}", e))).await;
                                                     let _ = tx_ui_clone.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                     break;
                                                }
                                            }
                                        }
                                    }
                                }
                            });

                            // Reader Task
                            let tx_ui_read = tx_ui.clone();
                            let tx_writer_read = tx_writer.clone();

                            tokio::spawn(async move {
                                let mut buffer = vec![0u8; 1024];
                                loop {
                                    let mut len_buf = [0u8; 4];
                                    match reader.read_exact(&mut len_buf).await {
                                        Ok(_) => {
                                            let len = u32::from_be_bytes(len_buf) as usize;
                                            if !(4..=65536).contains(&len) {
                                                let _ = tx_ui_read.send(UiEvent::Log(format!("Invalid PDU length: {}", len))).await;
                                                break;
                                            }

                                            if buffer.len() < len {
                                                buffer.resize(len, 0);
                                            }
                                            buffer[0..4].copy_from_slice(&len_buf);

                                            match reader.read_exact(&mut buffer[4..len]).await {
                                                Ok(_) => {
                                                    let cmd_id = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
                                                    match cmd_id {
                                                        CMD_BIND_RECEIVER_RESP |
                                                        CMD_BIND_TRANSMITTER_RESP |
                                                        CMD_BIND_TRANSCEIVER_RESP => {
                                                            match BindResponse::decode(&buffer[..len]) {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Bind Resp: {} ({})", resp.status_description, resp.command_status))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_SUBMIT_SM_RESP => {
                                                            match SubmitSmResponse::decode(&buffer[..len]) {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Submit Resp: {} (Msg ID: {})", resp.status_description, resp.message_id))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_SUBMIT_MULTI_SM_RESP => {
                                                            match SubmitMultiResp::decode(&buffer[..len]) {
                                                                Ok(resp) => {
                                                                    let mut log_msg = format!("SubmitMulti Resp: {} (Msg ID: {})", resp.status_description, resp.message_id);
                                                                    if !resp.unsuccess_smes.is_empty() {
                                                                        log_msg.push_str(&format!(" Failed: {}", resp.unsuccess_smes.len()));
                                                                    }
                                                                    let _ = tx_ui_read.send(UiEvent::Log(log_msg)).await;
                                                                }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_QUERY_SM_RESP => {
                                                            match QuerySmResponse::decode(&buffer[..len]) {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Query Resp: {} (State: {:?}, Err: {})", resp.status_description, resp.message_state, resp.error_code))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_CANCEL_SM_RESP => {
                                                            match CancelSmResponse::decode(&buffer[..len]) {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Cancel Resp: {}", resp.status_description))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_REPLACE_SM_RESP => {
                                                            match ReplaceSmResp::decode(&buffer[..len]) {
                                                                Ok(resp) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Replace Resp: {}", resp.status_description))).await; }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_DELIVER_SM => {
                                                            match DeliverSmRequest::decode(&buffer[..len]) {
                                                                Ok(req) => {
                                                                    let short_msg = String::from_utf8_lossy(&req.short_message).into_owned();
                                                                    let mut log_msg = format!("DeliverSM From: {} To: {}", req.source_addr, req.dest_addr);

                                                                    // Simple check for DLR based on esm_class or just content
                                                                    // In logic, we just log "Msg: ..."
                                                                    log_msg.push_str(&format!(" Msg: \"{}\"", short_msg));

                                                                    let _ = tx_ui_read.send(UiEvent::Log(log_msg)).await;

                                                                    // Send Response
                                                                    let resp = DeliverSmResponse::new(req.sequence_number, "ESME_ROK");
                                                                    let mut pdu = Vec::new();
                                                                    if resp.encode(&mut pdu).is_ok()
                                                                         && let Some(tx_w) = &tx_writer_read {
                                                                             let _ = tx_w.send(WriterCmd::Write(pdu)).await;
                                                                         }
                                                                }
                                                                Err(e) => { let _ = tx_ui_read.send(UiEvent::Log(format!("DeliverSM Parse Error: {:?}", e))).await; }
                                                            }
                                                        },
                                                        CMD_UNBIND_RESP => { // 0x80000006
                                                            let _ = tx_ui_read.send(UiEvent::Log("Unbind Response Received".to_string())).await;
                                                            let _ = tx_ui_read.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                            if let Some(tx_w) = &tx_writer_read {
                                                                let _ = tx_w.send(WriterCmd::Close).await;
                                                            }
                                                            break;
                                                        },
                                                        CMD_ENQUIRE_LINK_RESP | CMD_ENQUIRE_LINK => {
                                                             // Ignore
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
                                             let _ = tx_ui_read.send(UiEvent::Log("Connection closed by peer".to_string())).await;
                                             let _ = tx_ui_read.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                            break;
                                        }
                                    }
                                }
                            });

                            // Send Bind
                            let mode_enum = match bind_mode.as_str() {
                                "Transmitter" => BindMode::Transmitter,
                                "Receiver" => BindMode::Receiver,
                                "Transceiver" => BindMode::Transceiver,
                                _ => BindMode::Transceiver,
                            };

                            let mut pdu = Vec::new();
                            let req = BindRequest::new(
                                rand::thread_rng().gen_range(1..10000),
                                mode_enum,
                                system_id,
                                password
                            );



                            if req.encode(&mut pdu).is_ok() {
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
                        let req = UnbindRequest::new(rand::thread_rng().gen_range(1..10000));
                        let mut pdu = Vec::new();
                        if req.encode(&mut pdu).is_ok() {
                            let _ = tx.send(WriterCmd::Write(pdu)).await;
                        }
                     }
                }
                Cmd::SendMessage { source, src_ton, src_npi, dest, dest_ton, dest_npi, message, encoding, mode, pid, dcs, validity, dlr } => {
                     if let Some(tx) = &tx_writer {
                        let enc_enum = match encoding.as_str() {
                            "GSM 7-bit" => EncodingType::Gsm7Bit,
                            "Latin-1" => EncodingType::Latin1,
                            "UCS-2" => EncodingType::Ucs2,
                            _ => EncodingType::Gsm7Bit,
                        };

                        let mode_enum = match mode.as_str() {
                            "UDH" => SplitMode::Udh,
                            "SAR" => SplitMode::Sar,
                            "Payload" => SplitMode::Payload,
                            _ => SplitMode::Udh,
                        };

                        match MessageSplitter::split(message, enc_enum, mode_enum) {
                            Ok((parts, data_coding_auto)) => {
                                let total = parts.len();
                                let mut seq_num = rand::thread_rng().gen_range(1..10000) as u32;
                                let sar_ref_num = rand::thread_rng().r#gen::<u16>();

                                for (i, part) in parts.into_iter().enumerate() {
                                    // Parse destinations to check if it's a multi-submit
                                    let dests: Vec<&str> = dest.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

                                    if dests.len() > 1 {
                                        // SubmitMulti Logic
                                        let destinations: Vec<Destination> = dests.iter().map(|d| {
                                            Destination::SmeAddress {
                                                ton: Ton::from(dest_ton.parse::<u8>().unwrap_or(0)),
                                                npi: Npi::from(dest_npi.parse::<u8>().unwrap_or(0)),
                                                address: d.to_string(),
                                            }
                                        }).collect();

                                        let mut req = SubmitMulti::new(
                                            seq_num,
                                            source.clone(),
                                            destinations,
                                            if mode_enum == SplitMode::Payload { Vec::new() } else { part.clone() }
                                        );

                                        if mode_enum == SplitMode::Payload {
                                            req.optional_params.push(Tlv::new(tags::MESSAGE_PAYLOAD, part));
                                        }

                                        // Set Common Fields
                                        req.data_coding = dcs.parse().unwrap_or(data_coding_auto);
                                        if let Ok(pid_val) = pid.parse() { req.protocol_id = pid_val; }
                                        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                                        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
                                        req.validity_period = validity.clone();
                                        req.registered_delivery = if dlr { 1 } else { 0 };

                                        if mode_enum == SplitMode::Udh && total > 1 {
                                            req.esm_class = 0x40; // UDHI
                                        }

                                        if mode_enum == SplitMode::Sar && total > 1 {
                                            req.optional_params.push(Tlv::new_u16(tags::SAR_MSG_REF_NUM, sar_ref_num));
                                            req.optional_params.push(Tlv::new_u8(tags::SAR_TOTAL_SEGMENTS, total as u8));
                                            req.optional_params.push(Tlv::new_u8(tags::SAR_SEGMENT_SEQNUM, (i + 1) as u8));
                                        }

                                        let mut pdu = Vec::new();
                                        if let Err(e) = req.encode(&mut pdu) {
                                            let _ = tx_ui.send(UiEvent::Log(format!("Encode Error (Multi): {:?}", e))).await;
                                            continue;
                                        }

                                        let _ = tx.send(WriterCmd::Write(pdu)).await;
                                        let _ = tx_ui.send(UiEvent::Log(format!("Sent Multi-Seg {}/{} to {} dests", i+1, total, dests.len()))).await;

                                    } else {
                                        // SubmitSm Logic (Existing)
                                        let mut req = if mode_enum == SplitMode::Payload {
                                            // For Payload mode, the short_message field is empty,
                                            // and the content goes into the message_payload TLV.
                                            let mut r = SubmitSmRequest::new(
                                                seq_num,
                                                source.clone(),
                                                dest.clone(),
                                                Vec::new()
                                            );
                                            r.add_tlv(Tlv::new(tags::MESSAGE_PAYLOAD, part));
                                            r
                                        } else {
                                            SubmitSmRequest::new(
                                                seq_num,
                                                source.clone(),
                                                dest.clone(),
                                                part
                                            )
                                        };

                                        // Set fields
                                        req.data_coding = dcs.parse().unwrap_or(data_coding_auto);
                                        if let Ok(pid_val) = pid.parse() { req.protocol_id = pid_val; }
                                        // Type of Number & NPI mapping
                                        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                                        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
                                        req.dest_addr_ton = Ton::from(dest_ton.parse::<u8>().unwrap_or(0));
                                        req.dest_addr_npi = Npi::from(dest_npi.parse::<u8>().unwrap_or(0));

                                        // Validity
                                        req.validity_period = validity.clone();
                                        req.registered_delivery = if dlr { 1 } else { 0 };

                                        if mode_enum == SplitMode::Udh && total > 1 {
                                            req.esm_class = 0x40; // UDHI
                                        }

                                        if mode_enum == SplitMode::Sar && total > 1 {
                                            // SAR Mode: Add SAR TLVs for reconstruction at the receiver
                                            req.add_tlv(Tlv::new_u16(tags::SAR_MSG_REF_NUM, sar_ref_num));
                                            req.add_tlv(Tlv::new_u8(tags::SAR_TOTAL_SEGMENTS, total as u8));
                                            req.add_tlv(Tlv::new_u8(tags::SAR_SEGMENT_SEQNUM, (i + 1) as u8));
                                        }

                                        let mut pdu = Vec::new();
                                        if let Err(e) = req.encode(&mut pdu) {
                                            let _ = tx_ui.send(UiEvent::Log(format!("Encode Error: {:?}", e))).await;
                                            continue;
                                        }

                                        let _ = tx.send(WriterCmd::Write(pdu)).await;
                                        let _ = tx_ui.send(UiEvent::Log(format!("Sent Segment {}/{}", i+1, total))).await;
                                    }

                                     seq_num += 1;
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
                Cmd::QuerySm { msg_id, source, ton, npi } => {
                    if let Some(tx) = &tx_writer {
                        let mut req = QuerySmRequest::new(
                            rand::thread_rng().gen_range(1..10000),
                            msg_id,
                            source,
                        );
                        req.source_addr_ton = Ton::from(ton.parse::<u8>().unwrap_or(0));
                        req.source_addr_npi = Npi::from(npi.parse::<u8>().unwrap_or(0));

                        let mut pdu = Vec::new();
                        if req.encode(&mut pdu).is_ok() {
                            let _ = tx.send(WriterCmd::Write(pdu)).await;
                            let _ = tx_ui.send(UiEvent::Log("Sent QuerySm".to_string())).await;
                        }
                    }
                }
                Cmd::CancelSm { msg_id, source, src_ton, src_npi, dest, dest_ton, dest_npi } => {
                     if let Some(tx) = &tx_writer {
                        let mut req = CancelSmRequest::new(
                            rand::thread_rng().gen_range(1..10000),
                            msg_id,
                            source,
                            dest,
                        );
                        req.service_type = "SMPP".to_string();
                        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
                        req.dest_addr_ton = Ton::from(dest_ton.parse::<u8>().unwrap_or(0));
                        req.dest_addr_npi = Npi::from(dest_npi.parse::<u8>().unwrap_or(0));

                        let mut pdu = Vec::new();
                        if req.encode(&mut pdu).is_ok() {
                            let _ = tx.send(WriterCmd::Write(pdu)).await;
                            let _ = tx_ui.send(UiEvent::Log("Sent CancelSm".to_string())).await;
                        }
                     }
                }
                Cmd::ReplaceSm { msg_id, source, src_ton, src_npi, message } => {
                     if let Some(tx) = &tx_writer {
                        let mut req = ReplaceSm::new(
                            rand::thread_rng().gen_range(1..10000),
                            msg_id,
                            source,
                            message.into_bytes(),
                        );
                        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));

                        let mut pdu = Vec::new();
                        if req.encode(&mut pdu).is_ok() {
                            let _ = tx.send(WriterCmd::Write(pdu)).await;
                            let _ = tx_ui.send(UiEvent::Log("Sent ReplaceSm".to_string())).await;
                        }
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
            use_ssl,
        });
    });

    let tx_cmd_unbind = tx_cmd.clone();
    main_window.on_unbind(move || {
        let _ = tx_cmd_unbind.blocking_send(Cmd::Unbind);
    });

    // Calculate string length including correct Unicode character count
    main_window.on_string_length(|s| s.chars().count() as i32);

    let tx_cmd_send = tx_cmd.clone();
    main_window.on_send_message(
        move |src,
              src_ton,
              src_npi,
              dest,
              dest_ton,
              dest_npi,
              msg,
              enc,
              mode,
              pid,
              dcs,
              val,
              dlr| {
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
                dlr,
            });
        },
    );

    let tx_cmd_query = tx_cmd.clone();
    main_window.on_query_sm(move |msg_id, source, ton, npi| {
        let _ = tx_cmd_query.blocking_send(Cmd::QuerySm {
            msg_id: msg_id.into(),
            source: source.into(),
            ton: ton.into(),
            npi: npi.into(),
        });
    });

    let tx_cmd_cancel = tx_cmd.clone();
    main_window.on_cancel_sm(
        move |msg_id, source, src_ton, src_npi, dest, dest_ton, dest_npi| {
            let _ = tx_cmd_cancel.blocking_send(Cmd::CancelSm {
                msg_id: msg_id.into(),
                source: source.into(),
                src_ton: src_ton.into(),
                src_npi: src_npi.into(),
                dest: dest.into(),
                dest_ton: dest_ton.into(),
                dest_npi: dest_npi.into(),
            });
        },
    );

    let tx_cmd_replace = tx_cmd.clone();
    main_window.on_replace_sm(move |msg_id, source, src_ton, src_npi, msg| {
        let _ = tx_cmd_replace.blocking_send(Cmd::ReplaceSm {
            msg_id: msg_id.into(),
            source: source.into(),
            src_ton: src_ton.into(),
            src_npi: src_npi.into(),
            message: msg.into(),
        });
    });

    main_window.run()?;
    Ok(())
}
