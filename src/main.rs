#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use std::sync::Arc;
use tokio::sync::mpsc;

slint::include_modules!();
use slint::Model;

use rSMPP::app_logic::{Cmd, UiEvent, run_main_loop};
use rSMPP::network::RealNetworkConnector;

#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    let (tx_cmd, rx_cmd) = mpsc::channel::<Cmd>(100);
    let (tx_ui, mut rx_ui) = mpsc::channel::<UiEvent>(100);

    // Spawn the logic loop
    tokio::spawn(async move {
        let network_connector = Arc::new(RealNetworkConnector);
        run_main_loop(rx_cmd, tx_ui, network_connector).await;
    });

    let ui_handle = ui.as_weak();
    let _ui_update_handle = slint::spawn_local(async move {
        while let Some(event) = rx_ui.recv().await {
            let ui_weak = ui_handle.clone();
            let _ = slint::invoke_from_event_loop(move || match event {
                UiEvent::Log(msg) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        let mut logs: Vec<slint::SharedString> = ui.get_logs().iter().collect();
                        logs.push(msg.into());
                        if logs.len() > 100 {
                            logs.remove(0);
                        }
                        let model = std::rc::Rc::new(slint::VecModel::from(logs));
                        ui.set_logs(model.into());
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
        let _ = tx.blocking_send(Cmd::Connect {
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
        let _ = tx.blocking_send(Cmd::Unbind);
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
            let _ = tx.blocking_send(Cmd::SendMessage {
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
        let _ = tx.blocking_send(Cmd::QuerySm {
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
            let _ = tx.blocking_send(Cmd::CancelSm {
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
        let _ = tx.blocking_send(Cmd::ReplaceSm {
            msg_id: msg_id.into(),
            source: source.into(),
            src_ton: src_ton.into(),
            src_npi: src_npi.into(),
            message: message.into(),
        });
    });

    ui.on_string_length(move |s| s.len() as i32);

    ui.run()
}
