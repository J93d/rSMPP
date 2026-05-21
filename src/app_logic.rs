use rand::Rng;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

// SMPP Codec Imports
use smpp_codec::common::{
    CMD_BIND_RECEIVER_RESP, CMD_BIND_TRANSCEIVER_RESP, CMD_BIND_TRANSMITTER_RESP, CMD_DELIVER_SM,
    CMD_ENQUIRE_LINK, CMD_ENQUIRE_LINK_RESP, CMD_SUBMIT_MULTI_SM_RESP, CMD_SUBMIT_SM_RESP,
    CMD_UNBIND_RESP,
};
use smpp_codec::common::{CMD_CANCEL_SM_RESP, CMD_QUERY_SM_RESP, CMD_REPLACE_SM_RESP};
use smpp_codec::pdus::{
    BindResponse, CancelSmResponse, DeliverSmRequest, DeliverSmResponse, EnquireLinkRequest,
    EnquireLinkResponse, QuerySmResponse, ReplaceSmResp, SubmitMultiResp, SubmitSmResponse,
};

use crate::network::NetworkConnector;
use crate::pdu_factory::PduFactory;

#[derive(Debug, Clone)]
pub enum UiEvent {
    Log(String),
    ConnectionStatus(String, bool),
}

#[derive(Debug)]
pub enum Cmd {
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

#[derive(Debug)]
pub enum WriterCmd {
    Write(Vec<u8>),
    Close,
}

pub async fn run_main_loop(
    mut rx_cmd: mpsc::Receiver<Cmd>,
    tx_ui: mpsc::Sender<UiEvent>,
    network_connector: Arc<dyn NetworkConnector>,
) {
    let mut tx_writer: Option<mpsc::Sender<WriterCmd>> = None;
    let mut writer_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut reader_task: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(cmd) = rx_cmd.recv().await {
        // Clean up stale connection state if tasks have terminated
        if let Some(handle) = writer_task.as_ref()
            && handle.is_finished()
        {
            writer_task = None;
            tx_writer = None;
        }
        if let Some(handle) = reader_task.as_ref()
            && handle.is_finished()
        {
            reader_task = None;
            // Writer may still be running; signal it to close
            if let Some(tx) = tx_writer.take() {
                let _ = tx.try_send(WriterCmd::Close);
            }
        }

        match cmd {
            Cmd::Connect {
                ip,
                port,
                system_id,
                password,
                bind_mode,
                use_ssl,
            } => {
                let addr = format!("{}:{}", ip, port);

                // Shut down any existing connection tasks before reconnecting
                if let Some(tx) = tx_writer.take() {
                    let _ = tx.send(WriterCmd::Close).await;
                }
                if let Some(handle) = writer_task.take() {
                    handle.abort();
                }
                if let Some(handle) = reader_task.take() {
                    handle.abort();
                }

                match network_connector.connect(&ip, &port, use_ssl).await {
                    Ok((mut reader, mut writer)) => {
                        let _ = tx_ui.try_send(UiEvent::Log(format!(
                            "Connected to {} (SSL: {})",
                            addr, use_ssl
                        )));
                        let _ = tx_ui
                            .send(UiEvent::ConnectionStatus("Connected".to_string(), true))
                            .await;

                        let (tx_w, mut rx_w) = mpsc::channel::<WriterCmd>(100);
                        tx_writer = Some(tx_w.clone());

                        let tx_ui_clone = tx_ui.clone();
                        let w_handle = tokio::spawn(async move {
                            let mut interval = time::interval(Duration::from_secs(5));
                            // Skip the immediate first tick so the heartbeat doesn't
                            // fire before the bind request, and use Delay to avoid
                            // burst-firing missed ticks inside the select! loop.
                            interval.tick().await;
                            interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);
                            loop {
                                select! {
                                    cmd = rx_w.recv() => {
                                        match cmd {
                                            Some(w_cmd) => {
                                                match w_cmd {
                                                    WriterCmd::Write(data) => {
                                                        if let Err(e) = writer.write_all(&data).await {
                                                            let _ = tx_ui_clone.try_send(UiEvent::Log(format!("Write Error: {}", e)));
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
                                                 let _ = tx_ui_clone.try_send(UiEvent::Log(format!("Heartbeat Error: {}", e)));
                                                 let _ = tx_ui_clone.send(UiEvent::ConnectionStatus("Disconnected".to_string(), false)).await;
                                                 break;
                                            }
                                        }
                                    }
                                }
                            }
                        });
                        writer_task = Some(w_handle);

                        // Reader Task
                        let tx_ui_read = tx_ui.clone();
                        let tx_writer_read = tx_writer.clone();

                        let r_handle = tokio::spawn(async move {
                            // Timeout for individual PDU reads; prevents hanging on partial
                            // packets from a misbehaving or crashing SMSC.
                            const READ_TIMEOUT: Duration = Duration::from_secs(30);
                            let mut buffer = vec![0u8; 1024];
                            loop {
                                let mut len_buf = [0u8; 4];
                                // --- Read PDU header (4-byte length prefix) ---
                                let header_result =
                                    time::timeout(READ_TIMEOUT, reader.read_exact(&mut len_buf))
                                        .await;
                                let read_header = match header_result {
                                    Ok(inner) => inner,
                                    Err(_elapsed) => {
                                        let _ = tx_ui_read.try_send(UiEvent::Log(
                                            "Read timeout waiting for PDU header — disconnecting"
                                                .to_string(),
                                        ));
                                        let _ = tx_ui_read
                                            .send(UiEvent::ConnectionStatus(
                                                "Disconnected".to_string(),
                                                false,
                                            ))
                                            .await;
                                        if let Some(tx_w) = &tx_writer_read {
                                            let _ = tx_w.send(WriterCmd::Close).await;
                                        }
                                        break;
                                    }
                                };
                                match read_header {
                                    Ok(_) => {
                                        let len = u32::from_be_bytes(len_buf) as usize;
                                        if !(4..=65536).contains(&len) {
                                            let _ = tx_ui_read.try_send(UiEvent::Log(format!(
                                                "Invalid PDU length {len} — disconnecting"
                                            )));
                                            let _ = tx_ui_read
                                                .send(UiEvent::ConnectionStatus(
                                                    "Disconnected".to_string(),
                                                    false,
                                                ))
                                                .await;
                                            if let Some(tx_w) = &tx_writer_read {
                                                let _ = tx_w.send(WriterCmd::Close).await;
                                            }
                                            break;
                                        }

                                        if buffer.len() < len {
                                            buffer.resize(len, 0);
                                        }
                                        buffer[0..4].copy_from_slice(&len_buf);

                                        // --- Read PDU body (remaining bytes after length) ---
                                        let body_result = time::timeout(
                                            READ_TIMEOUT,
                                            reader.read_exact(&mut buffer[4..len]),
                                        )
                                        .await;
                                        let read_body = match body_result {
                                            Ok(inner) => inner,
                                            Err(_elapsed) => {
                                                let _ = tx_ui_read.try_send(UiEvent::Log(
                                                    format!(
                                                        "Read timeout waiting for PDU body \
                                                         ({} bytes expected) — disconnecting",
                                                        len - 4
                                                    ),
                                                ));
                                                let _ = tx_ui_read
                                                    .send(UiEvent::ConnectionStatus(
                                                        "Disconnected".to_string(),
                                                        false,
                                                    ))
                                                    .await;
                                                if let Some(tx_w) = &tx_writer_read {
                                                    let _ = tx_w.send(WriterCmd::Close).await;
                                                }
                                                break;
                                            }
                                        };
                                        match read_body {
                                            Ok(_) => {
                                                let cmd_id = u32::from_be_bytes([
                                                    buffer[4], buffer[5], buffer[6], buffer[7],
                                                ]);
                                                match cmd_id {
                                                    CMD_BIND_RECEIVER_RESP
                                                    | CMD_BIND_TRANSMITTER_RESP
                                                    | CMD_BIND_TRANSCEIVER_RESP => {
                                                        match BindResponse::decode(&buffer[..len]) {
                                                            Ok(resp) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Bind Resp: {} ({})",
                                                                        resp.status_description,
                                                                        resp.command_status
                                                                    )),
                                                                );
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_SUBMIT_SM_RESP => {
                                                        match SubmitSmResponse::decode(
                                                            &buffer[..len],
                                                        ) {
                                                            Ok(resp) => {
                                                                let _ = tx_ui_read.try_send(UiEvent::Log(format!("Submit Resp: {} (Msg ID: {})", resp.status_description, resp.message_id)));
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_SUBMIT_MULTI_SM_RESP => {
                                                        match SubmitMultiResp::decode(
                                                            &buffer[..len],
                                                        ) {
                                                            Ok(resp) => {
                                                                let mut log_msg = format!(
                                                                    "SubmitMulti Resp: {} (Msg ID: {})",
                                                                    resp.status_description,
                                                                    resp.message_id
                                                                );
                                                                if !resp.unsuccess_smes.is_empty() {
                                                                    log_msg.push_str(&format!(
                                                                        " Failed: {}",
                                                                        resp.unsuccess_smes.len()
                                                                    ));
                                                                }
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(log_msg),
                                                                );
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_QUERY_SM_RESP => {
                                                        match QuerySmResponse::decode(
                                                            &buffer[..len],
                                                        ) {
                                                            Ok(resp) => {
                                                                let _ = tx_ui_read.try_send(UiEvent::Log(format!("Query Resp: {} (State: {:?}, Err: {})", resp.status_description, resp.message_state, resp.error_code)));
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_CANCEL_SM_RESP => {
                                                        match CancelSmResponse::decode(
                                                            &buffer[..len],
                                                        ) {
                                                            Ok(resp) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Cancel Resp: {}",
                                                                        resp.status_description
                                                                    )),
                                                                );
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_REPLACE_SM_RESP => {
                                                        match ReplaceSmResp::decode(&buffer[..len])
                                                        {
                                                            Ok(resp) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Replace Resp: {}",
                                                                        resp.status_description
                                                                    )),
                                                                );
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(format!(
                                                                        "Parse Error: {:?}",
                                                                        e
                                                                    )),
                                                                );
                                                            }
                                                        }
                                                    }
                                                    CMD_DELIVER_SM => {
                                                        match DeliverSmRequest::decode(
                                                            &buffer[..len],
                                                        ) {
                                                            Ok(req) => {
                                                                let short_msg =
                                                                    String::from_utf8_lossy(
                                                                        &req.short_message,
                                                                    )
                                                                    .into_owned();
                                                                let mut log_msg = format!(
                                                                    "DeliverSM From: {} To: {}",
                                                                    req.source_addr, req.dest_addr
                                                                );

                                                                // Simple check for DLR based on esm_class or just content
                                                                // In logic, we just log "Msg: ..."
                                                                log_msg.push_str(&format!(
                                                                    " Msg: \"{}\"",
                                                                    short_msg
                                                                ));

                                                                let _ = tx_ui_read.try_send(
                                                                    UiEvent::Log(log_msg),
                                                                );

                                                                // Send Response
                                                                let resp = DeliverSmResponse::new(
                                                                    req.sequence_number,
                                                                    "ESME_ROK",
                                                                );
                                                                let mut pdu = Vec::new();
                                                                if resp.encode(&mut pdu).is_ok()
                                                                    && let Some(tx_w) =
                                                                        &tx_writer_read
                                                                {
                                                                    let _ = tx_w
                                                                        .send(WriterCmd::Write(pdu))
                                                                        .await;
                                                                }
                                                            }
                                                            Err(e) => {
                                                                let _ = tx_ui_read.try_send(UiEvent::Log(format!("DeliverSM Parse Error: {:?}", e)));
                                                            }
                                                        }
                                                    }
                                                    CMD_UNBIND_RESP => {
                                                        // 0x80000006
                                                        let _ = tx_ui_read.try_send(UiEvent::Log(
                                                            "Unbind Response Received".to_string(),
                                                        ));
                                                        let _ = tx_ui_read
                                                            .send(UiEvent::ConnectionStatus(
                                                                "Disconnected".to_string(),
                                                                false,
                                                            ))
                                                            .await;
                                                        if let Some(tx_w) = &tx_writer_read {
                                                            let _ =
                                                                tx_w.send(WriterCmd::Close).await;
                                                        }
                                                        break;
                                                    }
                                                    CMD_ENQUIRE_LINK_RESP => {
                                                        // Heartbeat acknowledgment — nothing to do
                                                    }
                                                    CMD_ENQUIRE_LINK => {
                                                        // Respond to server's heartbeat
                                                        let seq = u32::from_be_bytes([
                                                            buffer[12], buffer[13], buffer[14],
                                                            buffer[15],
                                                        ]);
                                                        let resp = EnquireLinkResponse::new(
                                                            seq, "ESME_ROK",
                                                        );
                                                        let mut pdu = Vec::new();
                                                        if resp.encode(&mut pdu).is_ok()
                                                            && let Some(tx_w) = &tx_writer_read
                                                        {
                                                            let _ = tx_w
                                                                .send(WriterCmd::Write(pdu))
                                                                .await;
                                                        }
                                                    }
                                                    _ => {
                                                        // Unknown command ID: log the raw bytes so
                                                        // the user can inspect the malformed PDU
                                                        // without the reader blocking.
                                                        let hex_preview: String = buffer
                                                            [..len.min(32)]
                                                            .iter()
                                                            .map(|b| format!("{b:02X}"))
                                                            .collect::<Vec<_>>()
                                                            .join(" ");
                                                        let _ = tx_ui_read.try_send(UiEvent::Log(
                                                            format!(
                                                                "Unknown PDU CmdID 0x{cmd_id:08X} \
                                                                 ({len} bytes): {hex_preview}"
                                                            ),
                                                        ));
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                let _ = tx_ui_read.try_send(UiEvent::Log(format!(
                                                    "Read Error (PDU body): {e}"
                                                )));
                                                let _ = tx_ui_read
                                                    .send(UiEvent::ConnectionStatus(
                                                        "Disconnected".to_string(),
                                                        false,
                                                    ))
                                                    .await;
                                                // Signal the writer task to stop
                                                if let Some(tx_w) = &tx_writer_read {
                                                    let _ = tx_w.send(WriterCmd::Close).await;
                                                }
                                                break;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx_ui_read.try_send(UiEvent::Log(format!(
                                            "Read Error (PDU header): {e}"
                                        )));
                                        let _ = tx_ui_read
                                            .send(UiEvent::ConnectionStatus(
                                                "Disconnected".to_string(),
                                                false,
                                            ))
                                            .await;
                                        // Signal the writer task to stop
                                        if let Some(tx_w) = &tx_writer_read {
                                            let _ = tx_w.send(WriterCmd::Close).await;
                                        }
                                        break;
                                    }
                                }
                            }
                        });
                        reader_task = Some(r_handle);

                        let pdu = PduFactory::create_bind_request(
                            rand::thread_rng().gen_range(1..10000),
                            &bind_mode,
                            &system_id,
                            &password,
                        );
                        let _ = tx_writer
                            .as_ref()
                            .unwrap()
                            .send(WriterCmd::Write(pdu))
                            .await;
                    }
                    Err(e) => {
                        let _ = tx_ui.try_send(UiEvent::Log(format!("Failed to connect: {}", e)));
                        let _ = tx_ui
                            .send(UiEvent::ConnectionStatus("Disconnected".to_string(), false))
                            .await;
                    }
                }
            }
            Cmd::Unbind => {
                if let Some(tx) = &tx_writer {
                    let _ = tx_ui.try_send(UiEvent::Log("Sending Unbind...".to_string()));
                    let pdu =
                        PduFactory::create_unbind_request(rand::thread_rng().gen_range(1..10000));
                    let _ = tx.send(WriterCmd::Write(pdu)).await;
                }
            }
            Cmd::SendMessage {
                source,
                src_ton,
                src_npi,
                dest,
                dest_ton,
                dest_npi,
                message,
                encoding,
                mode,
                pid,
                dcs,
                validity,
                dlr,
            } => {
                if let Some(tx) = &tx_writer {
                    let start_seq_num = rand::thread_rng().gen_range(1..10000) as u32;
                    match PduFactory::create_submit_pdus(
                        start_seq_num,
                        &source,
                        &src_ton,
                        &src_npi,
                        &dest,
                        &dest_ton,
                        &dest_npi,
                        &message,
                        &encoding,
                        &mode,
                        &pid,
                        &dcs,
                        &validity,
                        dlr,
                    ) {
                        Ok(pdus) => {
                            let total = pdus.len();
                            for (i, pdu) in pdus.into_iter().enumerate() {
                                let _ = tx.send(WriterCmd::Write(pdu)).await;
                                if dest.contains(',') {
                                    let _ = tx_ui.try_send(UiEvent::Log(format!(
                                        "Sent Multi-Seg {}/{}",
                                        i + 1,
                                        total
                                    )));
                                } else {
                                    let _ = tx_ui.try_send(UiEvent::Log(format!(
                                        "Sent Segment {}/{}",
                                        i + 1,
                                        total
                                    )));
                                }
                            }
                        }
                        Err(e) => {
                            let _ =
                                tx_ui.try_send(UiEvent::Log(format!("Error creating PDUs: {}", e)));
                        }
                    }
                } else {
                    let _ = tx_ui.try_send(UiEvent::Log("Not connected".to_string()));
                }
            }
            Cmd::QuerySm {
                msg_id,
                source,
                ton,
                npi,
            } => {
                if let Some(tx) = &tx_writer {
                    let pdu = PduFactory::create_query_sm_request(
                        rand::thread_rng().gen_range(1..10000),
                        &msg_id,
                        &source,
                        &ton,
                        &npi,
                    );
                    let _ = tx.send(WriterCmd::Write(pdu)).await;
                    let _ = tx_ui.try_send(UiEvent::Log("Sent QuerySm".to_string()));
                }
            }
            Cmd::CancelSm {
                msg_id,
                source,
                src_ton,
                src_npi,
                dest,
                dest_ton,
                dest_npi,
            } => {
                if let Some(tx) = &tx_writer {
                    let pdu = PduFactory::create_cancel_sm_request(
                        rand::thread_rng().gen_range(1..10000),
                        &msg_id,
                        &source,
                        &src_ton,
                        &src_npi,
                        &dest,
                        &dest_ton,
                        &dest_npi,
                    );
                    let _ = tx.send(WriterCmd::Write(pdu)).await;
                    let _ = tx_ui.try_send(UiEvent::Log("Sent CancelSm".to_string()));
                }
            }
            Cmd::ReplaceSm {
                msg_id,
                source,
                src_ton,
                src_npi,
                message,
            } => {
                if let Some(tx) = &tx_writer {
                    let pdu = PduFactory::create_replace_sm_request(
                        rand::thread_rng().gen_range(1..10000),
                        &msg_id,
                        &source,
                        &src_ton,
                        &src_npi,
                        &message,
                    );
                    let _ = tx.send(WriterCmd::Write(pdu)).await;
                    let _ = tx_ui.try_send(UiEvent::Log("Sent ReplaceSm".to_string()));
                }
            }
        }
    }
}
