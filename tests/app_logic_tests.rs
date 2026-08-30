use async_trait::async_trait;
use rSMPP::app_logic::{Cmd, UiEvent, run_main_loop};
use rSMPP::network::NetworkConnector;
use std::sync::{Arc, Mutex};
use tokio::io::duplex;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf, split};
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

// ---------------------------------------------------------------------------
// Mock Network Connector
// ---------------------------------------------------------------------------

/// A mock connector that uses an in-process duplex channel instead of a real
/// TCP connection.  The test has full control over the "server" side of the
/// stream so it can send arbitrary bytes, partial packets, or close the
/// connection at will.
struct MockNetworkConnector {
    /// Server-side halves are stashed here so the test can access them.
    server_side: Arc<Mutex<Option<(ReadHalf<DuplexStream>, WriteHalf<DuplexStream>)>>>,
}

#[async_trait]
impl NetworkConnector for MockNetworkConnector {
    async fn connect(
        &self,
        _ip: &str,
        _port: &str,
        _use_ssl: bool,
    ) -> Result<
        (
            Box<dyn AsyncRead + Unpin + Send>,
            Box<dyn AsyncWrite + Unpin + Send>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let (client, server) = duplex(65536);
        let (c_r, c_w) = split(client);
        let (s_r, s_w) = split(server);

        // Store server side so test can access it
        let mut guard = self.server_side.lock().unwrap();
        *guard = Some((s_r, s_w));

        Ok((Box::new(c_r), Box::new(c_w)))
    }
}

// ---------------------------------------------------------------------------
// Helper: drain UI events until a predicate is met or we time out.
// ---------------------------------------------------------------------------

/// Collect UI events until `pred` returns `Some(T)` or the wall-clock timeout
/// expires.  Returns `None` on timeout.
async fn drain_until<T, F>(rx: &mut mpsc::Receiver<UiEvent>, pred: F) -> Option<T>
where
    F: Fn(&UiEvent) -> Option<T>,
{
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(ev) = rx.recv().await {
                if let Some(v) = pred(&ev) {
                    return v;
                }
            }
        }
    })
    .await
    .ok()
}

// ---------------------------------------------------------------------------
// Test 1 – happy-path connection
// ---------------------------------------------------------------------------

/// Verifies that a normal connect sends a "Connected" log and a
/// `ConnectionStatus(true)` event.
#[tokio::test]
async fn test_connect_flow() {
    let (tx_cmd, rx_cmd) = mpsc::channel(10);
    let (tx_ui, mut rx_ui) = mpsc::channel(32);

    let server_storage = Arc::new(Mutex::new(None));
    let mock_network = Arc::new(MockNetworkConnector {
        server_side: server_storage.clone(),
    });

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, mock_network).await;
    });

    tx_cmd
        .send(Cmd::Connect {
            ip: "127.0.0.1".to_string(),
            port: "2775".to_string(),
            system_id: "user".to_string(),
            password: "pass".to_string(),
            bind_mode: "Transceiver".to_string(),
            use_ssl: false,
        })
        .await
        .expect("send connect cmd");

    // Expect a log line that contains "Connected to"
    let got_log = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::Log(msg) if msg.contains("Connected to") => Some(true),
        _ => None,
    })
    .await;
    assert!(got_log.is_some(), "expected 'Connected to' log");

    // Expect a ConnectionStatus(true)
    let got_status = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, true) => Some(true),
        _ => None,
    })
    .await;
    assert!(got_status.is_some(), "expected connected status event");
}

// ---------------------------------------------------------------------------
// Test 2 – PDU with invalid (too-large) length field disconnects cleanly
// ---------------------------------------------------------------------------

/// Sends a 4-byte header whose `command_length` field encodes a value that is
/// too large (> 65 536).  The reader task should log the problem and emit a
/// clean `ConnectionStatus(false)` without hanging.
#[tokio::test]
async fn test_invalid_pdu_length_disconnects() {
    let (tx_cmd, rx_cmd) = mpsc::channel(10);
    let (tx_ui, mut rx_ui) = mpsc::channel(32);

    let server_storage = Arc::new(Mutex::new(None));
    let mock_network = Arc::new(MockNetworkConnector {
        server_side: server_storage.clone(),
    });

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, mock_network).await;
    });

    tx_cmd
        .send(Cmd::Connect {
            ip: "127.0.0.1".to_string(),
            port: "2775".to_string(),
            system_id: "u".to_string(),
            password: "p".to_string(),
            bind_mode: "Transceiver".to_string(),
            use_ssl: false,
        })
        .await
        .expect("send connect cmd");

    // Wait until the connection is established
    drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, true) => Some(()),
        _ => None,
    })
    .await
    .expect("should connect");

    // Grab the server write-half and inject a PDU with length = 0xFFFF_FFFF
    let server_write = {
        let mut guard = server_storage.lock().unwrap();
        let (_, sw) = guard.take().expect("server side must be set");
        sw
    };
    let mut server_write = server_write;
    // command_length = 0xFFFF_FFFF — far exceeds the 65 536 cap
    let bad_len: u32 = 0xFFFF_FFFF;
    server_write
        .write_all(&bad_len.to_be_bytes())
        .await
        .expect("write bad length");

    // Reader must emit a disconnection event — no hang allowed
    let disconnected = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, false) => Some(true),
        _ => None,
    })
    .await;
    assert!(
        disconnected.is_some(),
        "invalid PDU length should trigger a clean disconnect"
    );
}

// ---------------------------------------------------------------------------
// Test 3 – truncated PDU body disconnects cleanly (no hang)
// ---------------------------------------------------------------------------

/// Sends a header that advertises a 32-byte PDU but then closes the
/// connection after only 4 bytes (i.e. the body is truncated).  The reader
/// should detect the EOF and disconnect cleanly rather than blocking forever.
#[tokio::test]
async fn test_truncated_pdu_body_disconnects() {
    let (tx_cmd, rx_cmd) = mpsc::channel(10);
    let (tx_ui, mut rx_ui) = mpsc::channel(32);

    let server_storage = Arc::new(Mutex::new(None));
    let mock_network = Arc::new(MockNetworkConnector {
        server_side: server_storage.clone(),
    });

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, mock_network).await;
    });

    tx_cmd
        .send(Cmd::Connect {
            ip: "127.0.0.1".to_string(),
            port: "2775".to_string(),
            system_id: "u".to_string(),
            password: "p".to_string(),
            bind_mode: "Transceiver".to_string(),
            use_ssl: false,
        })
        .await
        .expect("send connect cmd");

    // Wait until the connection is established
    drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, true) => Some(()),
        _ => None,
    })
    .await
    .expect("should connect");

    // Grab the server write-half
    let server_write = {
        let mut guard = server_storage.lock().unwrap();
        let (_, sw) = guard.take().expect("server side must be set");
        sw
    };
    let mut server_write = server_write;

    // Send header claiming 32-byte PDU, but send only 4 bytes total (just the
    // length field itself), then drop the writer so the client sees EOF.
    let claimed_len: u32 = 32;
    server_write
        .write_all(&claimed_len.to_be_bytes())
        .await
        .expect("write length header");
    // Drop the write half — this closes the server side, causing EOF.
    drop(server_write);

    // Reader must emit a disconnection event — must not hang
    let disconnected = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, false) => Some(true),
        _ => None,
    })
    .await;
    assert!(
        disconnected.is_some(),
        "truncated PDU body should trigger a clean disconnect"
    );
}

// ---------------------------------------------------------------------------
// Test 4 – unknown command ID is logged and the connection stays alive
// ---------------------------------------------------------------------------

/// Sends a well-formed 16-byte PDU whose command_id is 0xDEADBEEF (unknown).
/// The reader should log an informational message about the unknown command but
/// **not** disconnect: the session remains usable for subsequent packets.
#[tokio::test]
async fn test_unknown_command_id_logs_and_continues() {
    let (tx_cmd, rx_cmd) = mpsc::channel(10);
    let (tx_ui, mut rx_ui) = mpsc::channel(64);

    let server_storage = Arc::new(Mutex::new(None));
    let mock_network = Arc::new(MockNetworkConnector {
        server_side: server_storage.clone(),
    });

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, mock_network).await;
    });

    tx_cmd
        .send(Cmd::Connect {
            ip: "127.0.0.1".to_string(),
            port: "2775".to_string(),
            system_id: "u".to_string(),
            password: "p".to_string(),
            bind_mode: "Transceiver".to_string(),
            use_ssl: false,
        })
        .await
        .expect("send connect cmd");

    // Wait until the connection is established
    drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, true) => Some(()),
        _ => None,
    })
    .await
    .expect("should connect");

    // Grab the server write-half
    let server_write = {
        let mut guard = server_storage.lock().unwrap();
        let (_, sw) = guard.take().expect("server side must be set");
        sw
    };
    let mut server_write = server_write;

    // Build a minimal 16-byte PDU with command_id = 0xDEAD_BEEF
    let mut pdu = [0u8; 16];
    pdu[0..4].copy_from_slice(&16u32.to_be_bytes()); // command_length
    pdu[4..8].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes()); // command_id
    // command_status and sequence_number remain zero
    server_write
        .write_all(&pdu)
        .await
        .expect("write unknown PDU");

    // The reader should log about the unknown command ID
    let got_unknown_log = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::Log(msg) if msg.contains("Unknown PDU") || msg.contains("DEADBEEF") => Some(true),
        _ => None,
    })
    .await;
    assert!(
        got_unknown_log.is_some(),
        "unknown command id should produce an informational log"
    );

    // The connection must NOT have been torn down
    // (Give a short window; a disconnect event would arrive promptly if sent)
    let unexpected_disconnect = timeout(Duration::from_millis(300), async {
        loop {
            match rx_ui.recv().await {
                Some(UiEvent::ConnectionStatus(_, false)) => return true,
                Some(_) => continue,
                None => return false,
            }
        }
    })
    .await;

    assert!(
        unexpected_disconnect.is_err() || unexpected_disconnect == Ok(false),
        "an unknown command ID must NOT disconnect the session"
    );
}

// ---------------------------------------------------------------------------
// Test 5 – connection failure is reported
// ---------------------------------------------------------------------------

/// A connector that always fails; verifies that the UI receives a log entry
/// describing the failure and a `ConnectionStatus(false)` event.
struct FailingConnector;

#[async_trait]
impl NetworkConnector for FailingConnector {
    async fn connect(
        &self,
        _ip: &str,
        _port: &str,
        _use_ssl: bool,
    ) -> Result<
        (
            Box<dyn AsyncRead + Unpin + Send>,
            Box<dyn AsyncWrite + Unpin + Send>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("simulated connection failure".into())
    }
}

#[tokio::test]
async fn test_connection_failure_is_reported() {
    let (tx_cmd, rx_cmd) = mpsc::channel(10);
    let (tx_ui, mut rx_ui) = mpsc::channel(32);

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, Arc::new(FailingConnector)).await;
    });

    tx_cmd
        .send(Cmd::Connect {
            ip: "127.0.0.1".to_string(),
            port: "2775".to_string(),
            system_id: "u".to_string(),
            password: "p".to_string(),
            bind_mode: "Transceiver".to_string(),
            use_ssl: false,
        })
        .await
        .expect("send connect cmd");

    // Expect a log that mentions "Failed to connect"
    let got_err = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::Log(msg) if msg.contains("Failed to connect") => Some(true),
        _ => None,
    })
    .await;
    assert!(got_err.is_some(), "should log a connection failure");

    // And a ConnectionStatus(false)
    let got_disconnected = drain_until(&mut rx_ui, |ev| match ev {
        UiEvent::ConnectionStatus(_, false) => Some(true),
        _ => None,
    })
    .await;
    assert!(
        got_disconnected.is_some(),
        "failed connect should emit ConnectionStatus(false)"
    );
}
