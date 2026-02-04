# Compiling and Running rSMPP on Linux

## Prerequisites

Start by installing Rust (if not already installed):
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Install System Dependencies
rSMPP uses [Slint](https://slint.dev/) for its GUI, which requires specific system libraries on Linux.

**Ubuntu/Debian:**
```sh
sudo apt-get install libfontconfig1-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev pkg-config
```

**Fedora:**
```sh
sudo dnf install fontconfig-devel libxcb-devel libxkbcommon-devel openssl-devel
```

**Arch Linux:**
```sh
sudo pacman -S fontconfig libxcb libxkbcommon openssl
```

## Compilation

Navigate to the project directory and build:
```sh
cargo build --release
```

The executable will be located at:
`target/release/rSMPP`

## Running

```sh
./target/release/rSMPP
```
**Note:** The application icon will appear in the window title bar and taskbar automatically, as it is embedded in the application binary.
