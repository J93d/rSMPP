use rand::Rng;
use std::io::{self, Write};
use std::io::{Error, ErrorKind, Result};

use crate::smpp_error_codes;

const BIND_TRANSMITTER_CMD_ID: u32 = 0x00000002;
const COMMAND_STATUS_OK: u32 = 0x00000000;
const SMPP_INTERFACE_VERSION: u8 = 3;
const ADDR_TON_UNKNOWN: u8 = 1;
const ADDR_NPI_ISDN: u8 = 1;
pub type BindTransmitterResult = std::result::Result<Vec<u8>, BindTransmitterError>;

#[derive(Debug, thiserror::Error)]
pub enum BindTransmitterError {
    #[error("System ID is too long (max 16 bytes): {0}")]
    SystemIdTooLong(usize),
    #[error("Password is too long (max 9 bytes): {0}")]
    PasswordTooLong(usize),
    #[error("IO error occurred: {0}")]
    IoError(#[from] io::Error),
}

#[derive(Debug, Clone)]
pub struct BindTransmitterBuilder {
    pub system_id: String,
    pub password: String,
    pub system_type: String,
}

impl Default for BindTransmitterBuilder {
    fn default() -> Self {
        Self {
            system_id: String::new(),
            password: String::new(),
            system_type: String::new(),
        }
    }
}

impl BindTransmitterBuilder {
    pub fn new(system_id: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            system_id: system_id.into(),
            password: password.into(),
            system_type: String::new(),
        }
    }

    pub fn with_system_type(mut self, system_type: impl Into<String>) -> Self {
        self.system_type = system_type.into();
        self
    }
}

#[derive(Debug, Clone)]
pub struct BindTransmitterResponse {
    pub command_status: u32,
    pub status_name: String,
}

pub struct BindTransmitter;

impl BindTransmitter {
    pub async fn bind_transmitter_async(params: BindTransmitterBuilder) -> BindTransmitterResult {
        let system_id_bytes = params.system_id.as_bytes();
        let password_bytes = params.password.as_bytes();
        let system_type_bytes = params.system_type.as_bytes();

        if system_id_bytes.len() > 16 {
            return Err(BindTransmitterError::SystemIdTooLong(system_id_bytes.len()));
        }

        if password_bytes.len() > 9 {
            return Err(BindTransmitterError::PasswordTooLong(password_bytes.len()));
        }

        let sequence_number = rand::thread_rng().gen_range(1..=4294967294u32);

        let command_length = 19 + system_id_bytes.len() + password_bytes.len() + 
                            system_type_bytes.len() + 4;

        let mut pdu = Vec::with_capacity(command_length);

        // Write PDU header (16 bytes)
        pdu.write_all(&(command_length as u32).to_be_bytes())?;
        pdu.write_all(&BIND_TRANSMITTER_CMD_ID.to_be_bytes())?;
        pdu.write_all(&COMMAND_STATUS_OK.to_be_bytes())?;
        pdu.write_all(&sequence_number.to_be_bytes())?;

        pdu.write_all(system_id_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(password_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(system_type_bytes)?;
        pdu.write_all(&[0u8])?;

        pdu.write_all(&[SMPP_INTERFACE_VERSION])?;
        pdu.write_all(&[ADDR_TON_UNKNOWN])?;
        pdu.write_all(&[ADDR_NPI_ISDN])?;
        pdu.write_all(&[0u8])?;

        Ok(pdu)
    }

    pub async fn parse_bind_transmitter_resp(buffer: &[u8]) -> Result<BindTransmitterResponse> {
        if buffer.len() < 16 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "PDU buffer too short, minimum 16 bytes required for header",
            ));
        }

        let status_bytes = [buffer[4], buffer[5], buffer[6], buffer[7]];
        let command_status = u32::from_be_bytes(status_bytes);

        let status_name = smpp_error_codes::error_codes(command_status);

        Ok(BindTransmitterResponse {
            command_status,
            status_name,
        })
    }
}
