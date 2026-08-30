#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use std::sync::Arc;
use tokio::sync::mpsc;

slint::include_modules!();

use rSMPP::app_logic::{Cmd, UiEvent, run_main_loop};
use rSMPP::network::RealNetworkConnector;

/// Maximum number of log lines retained in the UI terminal.
const MAX_LOG_LINES: u32 = 500;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    let (tx_cmd, rx_cmd) = mpsc::channel::<Cmd>(100);
    let (tx_ui, mut rx_ui) = mpsc::channel::<UiEvent>(100);

    // Spawn the logic loop
    let logic_handle = tokio::spawn(async move {
        let network_connector = Arc::new(RealNetworkConnector);
        run_main_loop(rx_cmd, tx_ui, network_connector).await;
    });

    let ui_handle = ui.as_weak();
    let line_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let _ui_update_handle = slint::spawn_local(async move {
        let line_count = line_count.clone();
        while let Some(event) = rx_ui.recv().await {
            let ui_weak = ui_handle.clone();
            let line_count = line_count.clone();
            let _ = slint::invoke_from_event_loop(move || match event {
                UiEvent::Log(msg) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        let mut current = ui.get_log_text().to_string();
                        current.push_str("> ");
                        current.push_str(&msg);
                        current.push('\n');
                        let count =
                            line_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        // Trim the oldest line when exceeding the cap
                        if count > MAX_LOG_LINES {
                            if let Some(pos) = current.find('\n') {
                                current = current[pos + 1..].to_string();
                            }
                            line_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        ui.set_log_text(current.into());
                    }
                }
                UiEvent::ConnectionStatus(status, connected) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_connection_status(status.into());
                        ui.set_is_connected(connected);
                    }
                }
            });
        }
    });

    // Connect Link
    let tx = tx_cmd.clone();
    ui.on_connect(move |ip, port, sys_id, pass, mode, start_tls| {
        let _ = tx.try_send(Cmd::Connect {
            ip: ip.into(),
            port: port.into(),
            system_id: sys_id.into(),
            password: pass.into(),
            bind_mode: mode.into(),
            use_ssl: start_tls,
        });
    });

    // Unbind Link
    let tx = tx_cmd.clone();
    ui.on_unbind(move || {
        let _ = tx.try_send(Cmd::Unbind);
    });

    // Send Message
    let tx = tx_cmd.clone();
    ui.on_send_message(
        move |source,
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
              validity,
              dlr| {
            let _ = tx.try_send(Cmd::SendMessage {
                source: source.into(),
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
                validity: validity.into(),
                dlr,
            });
        },
    );

    // Query SM
    let tx = tx_cmd.clone();
    ui.on_query_sm(move |msg_id, source, ton, npi| {
        let _ = tx.try_send(Cmd::QuerySm {
            msg_id: msg_id.into(),
            source: source.into(),
            ton: ton.into(),
            npi: npi.into(),
        });
    });

    // Cancel SM
    let tx = tx_cmd.clone();
    ui.on_cancel_sm(
        move |msg_id, source, src_ton, src_npi, dest, dest_ton, dest_npi| {
            let _ = tx.try_send(Cmd::CancelSm {
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

    // Replace SM
    let tx = tx_cmd.clone();
    ui.on_replace_sm(move |msg_id, source, src_ton, src_npi, message| {
        let _ = tx.try_send(Cmd::ReplaceSm {
            msg_id: msg_id.into(),
            source: source.into(),
            src_ton: src_ton.into(),
            src_npi: src_npi.into(),
            message: message.into(),
        });
    });

    ui.on_string_length(move |s| s.len().min(i32::MAX as usize) as i32);

    let res = ui.run();

    // FINDING-06: Await logic handle to monitor for panics
    if let Err(e) = logic_handle.await {
        eprintln!("Logic loop task panicked: {}", e);
    }

    res
}
