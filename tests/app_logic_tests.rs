use async_trait::async_trait;
use rSMPP::app_logic::{Cmd, UiEvent, run_main_loop};
use rSMPP::network::NetworkConnector;
use std::sync::{Arc, Mutex};
use tokio::io::duplex;
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadHalf, WriteHalf, split};
use tokio::sync::mpsc;

// Mock Network Connector
struct MockNetworkConnector {
    // Store server side so test can access it
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
        let (client, server) = duplex(1024);
        let (c_r, c_w) = split(client);
        let (s_r, s_w) = split(server);

        // Store server side so test can access it
        let mut guard = self.server_side.lock().unwrap();
        *guard = Some((s_r, s_w));

        Ok((Box::new(c_r), Box::new(c_w)))
    }
}

#[tokio::test]
async fn test_connect_flow() {
    let (tx_cmd, rx_cmd) = mpsc::channel(1);
    let (tx_ui, mut rx_ui) = mpsc::channel(1);

    let server_storage = Arc::new(Mutex::new(None));
    let mock_network = Arc::new(MockNetworkConnector {
        server_side: server_storage.clone(),
    });

    tokio::spawn(async move {
        run_main_loop(rx_cmd, tx_ui, mock_network).await;
    });

    // Send Connect Command
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
        .unwrap();

    // Verify UI Logs "Connected"
    let event = rx_ui.recv().await.unwrap();
    match event {
        UiEvent::Log(msg) => assert!(msg.contains("Connected to")),
        _ => panic!("Expected Log event"),
    }

    let event = rx_ui.recv().await.unwrap();
    match event {
        UiEvent::ConnectionStatus(status, connected) => {
            assert_eq!(status, "Connected");
            assert!(connected);
        }
        _ => panic!("Expected ConnectionStatus event"),
    }
}
