# rSMPP Client

A modern, high-performance, and secure SMPP v3.4 client built with Rust and Slint UI.

## 🚀 Key Features

### 🖥️ Modern User Interface
-   **Built with Slint**: Lightweight, responsive, and native-looking UI.
-   **Dark Theme**: Sleek, professional dark mode design.
-   **Layout Optimized**: "Use every pixel" philosophy with compact, aligned controls.
-   **Responsive**: Adapts perfectly to different window sizes (min-width derived).

### 🔒 Security & Connectivity
-   **TLS 1.2 / 1.3 Support**: Secure connections using `rustls`.
-   **Flexible SSL**: "Use SSL" toggle for secure vs plain TCP connections.
-   **Dev-Friendly**: Includes a "Dangerous Verifier" mode to skip certificate validation (great for local testing/self-signed certs).
-   **Graceful Disconnect**: Dedicated Unbind support for clean session termination.

### 📨 Advanced Messaging
-   **Powered by**: `smpp-codec` (Comprehensive PDU library)
-   **Encoding Support**: 
    -   **GSM 7-bit**: Standard SMS encoding (basic + extended charset).
    -   **Latin-1 (8-bit)**: For binary or Western European languages.
    -   **UCS-2 (16-bit)**: Full support for Emoji and international scripts.
-   **Long Message Handling**:
    -   **UDH (User Data Header)**: Standard concatenation.
    -   **SAR (Segmentation and Reassembly)**: Optional TLV-based splitting.
    -   **Message Payload**: Send up to 64kb as a single payload TLV.
-   **Delivery Receipts (DLR)**: Request and view detailed Delivery Reports.

### 📊 Robust Logging
-   **Live Transaction Log**: Real-time view of all sent and received PDUs.
-   **Detailed DeliverSM**: Logs Sender, Recipient, Message Content, and specific DLR status/IDs.
-   **PDU Inspection**: Hex dumps and detailed error reporting for debugging.

## 🛠️ Getting Started

### Prerequisites
-   [Rust Toolchain](https://rustup.rs/) (cargo) installed.

### Run the Application
```bash
cargo run --release
```

### Linux Users
Please refer to [COMPILING_ON_LINUX.md](./COMPILING_ON_LINUX.md) for platform-specific dependencies and build instructions.

### Configuration
1.  **Server Config**: Enter Host IP and Port (default 2775).
2.  **Credentials**: Enter System ID and Password.
3.  **Bind Mode**: Choose Transceiver (Send/Recv), Information Receiver, or Transmitter.
4.  **SSL**: Check "Use SSL" for secure connections.
5.  **Connect**: Click to bind to the server.

## 🏗️ Tech Stack
-   **Language**: Rust 🦀
-   **UI Framework**: [Slint](https://slint.dev/)
-   **Async Runtime**: Tokio
-   **TLS**: tokio-rustls / rustls
-   **Protocol**: smpp-codec

## 📝 License
Apache 2.0

## 🤖 Disclaimer
This project was co-developed with **Google Gemini**, an advanced AI assistant, which generated the majority of the code, UI design, and documentation based on user requirements and prompts.
