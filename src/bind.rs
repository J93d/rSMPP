use rand::Rng;
use std::io::{self, Write};
use std::io::{Error, ErrorKind, Result};

use crate::common::{self, command_id};

/// Enum representing the bind mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BindMode {
    Receiver,
    Transmitter,
    Transceiver,
}

impl BindMode {
    pub fn command_id(&self) -> u32 {
        match self {
            BindMode::Receiver => command_id::BIND_RECEIVER,
            BindMode::Transmitter => command_id::BIND_TRANSMITTER,
            BindMode::Transceiver => command_id::BIND_TRANSCEIVER,
        }
    }
}

pub type BindResult = std::result::Result<Vec<u8>, BindError>;

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("System ID is too long (max 16 bytes): {0}")]
    SystemIdTooLong(usize),
    #[error("Password is too long (max 9 bytes): {0}")]
    PasswordTooLong(usize),
    #[error("IO error occurred: {0}")]
    IoError(#[from] io::Error),
}

#[derive(Debug, Clone)]
pub struct BindBuilder {
    pub system_id: String,
    pub password: String,
    pub system_type: String,
    pub mode: BindMode,
}

impl BindBuilder {
    pub fn new(mode: BindMode, system_id: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            system_id: system_id.into(),
            password: password.into(),
            system_type: String::new(),
            mode,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BindResponse {
    pub command_status: u32,
    pub status_name: String,
}

pub struct Bind;

impl Bind {
    pub async fn bind_async(params: BindBuilder) -> BindResult {
        let system_id_bytes = params.system_id.as_bytes();
        let password_bytes = params.password.as_bytes();
        let system_type_bytes = params.system_type.as_bytes();

        if system_id_bytes.len() > 16 {
            return Err(BindError::SystemIdTooLong(system_id_bytes.len()));
        }

        if password_bytes.len() > 9 {
            return Err(BindError::PasswordTooLong(password_bytes.len()));
        }

        let sequence_number = rand::thread_rng().gen_range(1..=4294967294u32);

        let command_length = 19 + system_id_bytes.len() + password_bytes.len() + 
                            system_type_bytes.len() + 4;

        let mut pdu = Vec::with_capacity(command_length);

        // Write PDU header (16 bytes)
        pdu.write_all(&(command_length as u32).to_be_bytes())?;
        pdu.write_all(&params.mode.command_id().to_be_bytes())?;
        pdu.write_all(&common::COMMAND_STATUS_OK.to_be_bytes())?;
        pdu.write_all(&sequence_number.to_be_bytes())?;

        pdu.write_all(system_id_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(password_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(system_type_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(&[common::SMPP_INTERFACE_VERSION])?;
        pdu.write_all(&[common::ton::UNKNOWN])?;
        pdu.write_all(&[common::npi::ISDN])?;
        pdu.write_all(&[0u8])?;

        Ok(pdu)
    }

    pub async fn parse_bind_resp(buffer: &[u8]) -> Result<BindResponse> {
        if buffer.len() < 16 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "PDU buffer too short, minimum 16 bytes required for header",
            ));
        }

        let status_bytes = [buffer[4], buffer[5], buffer[6], buffer[7]];
        let command_status = u32::from_be_bytes(status_bytes);

        let status_name = common::get_status_description(command_status);

        Ok(BindResponse {
            command_status,
            status_name,
        })
    }
}
