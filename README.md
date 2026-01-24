# rSMPP Client

A Rust-based SMPP (Short Message Peer-to-Peer) client application featuring a modern, dark-themed GUI built with [Slint](https://slint.dev).

## Features

-   **Modern GUI**: A responsive, dark-themed user interface for easy interaction.
-   **SMPP Protocol Support**:
    -   Bind Transmitter (async)
    -   Submit SM (Short Message)
    -   Comprehensive list of SMPP v3.4/v5.0 Error Codes
-   **Real-time Logging**: In-app console to view PDU transmission logs and server responses.
-   **Async Architecture**: Built on `tokio` for efficient, non-blocking asynchronous network operations, with a separate thread for the UI event loop to ensure responsiveness.

## Application Structure

-   `src/main.rs`: Entry point. Handles the Slint UI event loop and spawns the `tokio` runtime for SMPP network operations (Connect, Send Message).
-   `src/bind_transmitter.rs`: Implementation of the SMPP Bind Transmitter PDU and response parsing.
-   `src/submit_sm.rs`: Implementation of the Submit SM PDU creation and response parsing.
-   `src/smpp_error_codes.rs`: Mapping of hex status codes to human-readable SMPP error strings (e.g., `ESME_ROK`, `ESME_RINVPASWD`).
-   `ui/appwindow.slint`: Declarative UI definition using Slint.

## Prerequisites

-   Time for correct Rust installation use [rustup](https://rustup.rs/).
-   A working SMPP server (SMSC) or simulator to connect to.

## Building and Running

1.  **Build the project**:
    ```bash
    cargo build --release
    ```

2.  **Run the application**:
    ```bash
    cargo run --release
    ```

## Usage

1.  **Connection**:
    -   Launch the app.
    -   Enter the SMSC **IP Address** and **Port**.
    -   Enter your **System ID** and **Password**.
    -   Click **Connect**.
    -   Check the *Logs* panel for "Sent Bind Transmitter PDU" and "Bind Response: ESME_ROK".

2.  **Sending SMS**:
    -   Once connected, navigate to the "Send Message" panel.
    -   Enter the **Source Address** (Sender ID) and **Destination Address**.
    -   Type your message.
    -   Click **Send SMS**.
    -   Verify the status in the *Logs* panel.

## Development Notes

-   **Runtime Management**: The application manually manages a `tokio::runtime::Runtime` to allow the Slint UI (which blocks the main thread) to coexist with async network tasks.
-   **Error Handling**: Uses `thiserror` for typed errors and `anyhow` for top-level error management.

## Future Roadmap

-   GSM 7-bit, 8-bit, and 16-bit encoding support.
-   Long message support (Concatenation via UDH/SAR/Payload).
-   Delivery Receipt handling.
