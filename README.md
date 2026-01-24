# rSMPP Client

A Rust-based SMPP (Short Message Peer-to-Peer) client application featuring a modern, dark-themed GUI built with [Slint](https://slint.dev).

## Features

-   **Modern GUI**: A responsive, dark-themed user interface for easy interaction.
-   **SMPP Protocol Support**:
    -   Bind Receiver, Transmitter, Transceiver (Unified Async Module)
    -   Submit SM (Short Message)
    -   Deliver SM (Receipt Handling)
    -   Enquire Link (Heartbeat)
    -   Comprehensive list of SMPP v3.4/v5.0 Error Codes
-   **Advanced Message Handling**:
    -   **GSM Encoding**: 7-bit, 8-bit (Latin-1), 16-bit (UCS-2).
    -   **Concatenation**: Send long messages via UDH, SAR, or Payload.
-   **Real-time Logging**: In-app console to view PDU transmission logs and server responses.
-   **Async Architecture**: Built on `tokio` for efficient, non-blocking asynchronous network operations.

## Application Structure

-   `src/main.rs`: Entry point. Handles the UI and Async runtime.
-   `src/bind.rs`: Unified Bind logic (Rx/Tx/Trx).
-   `src/submit_sm.rs`: Submit SM and PDU creation/splitting logic.
-   `src/deliver_sm.rs`: Deliver SM handling logic.
-   `src/common.rs`: Shared constants and Error code mapping.
-   `src/gsm_encoding.rs`: Character set encoding logic.
-   `ui/appwindow.slint`: UI definition.

## Prerequisites

-   Latest [Rust](https://rustup.rs/) stable toolchain.
-   A working SMPP server (SMSC) or simulator.

## Building and Running

1.  **Build the project**:
    ```bash
    cargo build --release
    ```

2.  **Run the application**:
    ```bash
    cargo run --release
    ```

## Antivirus / Defender Issues

If **Microsoft Defender** or other antivirus software flags the executable:
1.  **This is a False Positive**: The application is unsigned (does not have a digital code signing certificate), and it opens network connections (SMPP), which can trigger heuristic detection algorithms.
2.  **Add an Exclusion**: Go to **Windows Security > Virus & threat protection > Manage settings > Exclusions** and add the `rSMPP.exe` or the build folder.
3.  **Metadata**: The latest build includes strict file metadata to help mitigate "unknown publisher" heuristics, but without a paid certificate, flags may still occur on new machines.

## Usage

1.  **Connection**:
    -   Enter SMSC credentials.
    -   Select **Bind Mode** (Transmitter, Receiver, Transceiver).
    -   Click **Connect**.
2.  **Sending SMS**:
    -   Enter Sender, Receiver, and Message.
    -   **Encoding**: Choose GSM 7-bit (Default), Latin-1, or UCS-2.
    -   **Multipart Mode**: Choose logic for long messages (UDH is most common).
    -   Click **Send SMS**.

## Future Roadmap

-   Delivery Receipt (DLR) handling.
-   Store and Forward queueing.
