use std::io::{self, Write};
use regex::Regex;
use once_cell::sync::Lazy;

// Import error codes from the separate module
use crate::common::command_id; 

/// SMPP Protocol Data Unit (PDU) for deliver_sm operation
/// 
/// This module provides functionality to parse SMPP deliver_sm PDUs
/// received from an SMPP server and generate appropriate responses.

// SMPP command IDs (Now using common::command_id)

static MSG_ID: Lazy<Regex> = Lazy::new(|| {Regex::new(r"id:\d+").unwrap()});
static MSG_STATUS: Lazy<Regex> = Lazy::new(|| {Regex::new(r"stat:\S+").unwrap()});

/// Represents a parsed deliver_sm PDU
#[derive(Debug, Clone, PartialEq)]
pub struct DeliverSm {
    /// Sequence number from the original PDU
    pub sequence_number: u32,
    /// The originating address (sender)
    pub originator_address: String,
    /// The destination address (recipient)  
    pub destination_address: String,
    /// The message content
    pub message: String,
    /// The msg_id of SubmitSM
    pub returned_msg_id: u64,
    /// The status of SubmitSM
    pub returned_msg_status: String,
}

/// Errors that can occur when parsing a deliver_sm PDU
#[derive(Debug, thiserror::Error)]
pub enum DeliverSmError {
    #[error("Buffer too short: expected at least {expected} bytes, got {actual}")]
    BufferTooShort { expected: usize, actual: usize },
    #[error("Invalid UTF-8 in originator address")]
    InvalidOriginatorAddress,
    #[error("Invalid UTF-8 in destination address")]
    InvalidDestinationAddress,
    #[error("Invalid UTF-8 in message content")]
    InvalidMessageContent,
    #[error("Null terminator not found")]
    NullTerminatorNotFound,
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
}

/// Result type for deliver_sm operations
pub type DeliverSmResult<T> = Result<T, DeliverSmError>;

/// Parses an SMPP deliver_sm PDU from a buffer asynchronously
/// 
/// This function extracts the sequence number, originator address, 
/// destination address, and message content from a deliver_sm PDU.
/// 
/// # Arguments
/// * `buffer` - The raw PDU buffer received from the network
/// 
/// # Returns
/// * `Ok(DeliverSm)` - Successfully parsed deliver_sm data
/// * `Err(DeliverSmError)` - If parsing fails
/// 
/// # Example
/// ```rust
/// let buffer = vec![/* PDU bytes */];
/// match parse_deliver_sm_async(&buffer).await {
///     Ok(deliver_sm) => {
///         println!("From: {}", deliver_sm.originator_address);
///         println!("Message: {}", deliver_sm.message);
///     }
///     Err(e) => println!("Parse error: {}", e),
/// }
/// ```
pub async fn parse_deliver_sm_async(buffer: &[u8]) -> DeliverSmResult<DeliverSm> {
    // safe_log!(trace,"The DeliverSM Bytes received: {:?}", buffer); 
    // Validate minimum buffer size
    if buffer.len() < 16 {
        return Err(DeliverSmError::BufferTooShort {
            expected: 16,
            actual: buffer.len(),
        });
    }
    
    // Extract sequence number from bytes 12-15 (since the length is not passed on to this module)
    let sequence_number = u32::from_be_bytes([
        buffer[12], buffer[13], buffer[14], buffer[15]
    ]);

    // Find originator address (starts at byte 19, null-terminated)
    if buffer.len() < 20 {
        return Err(DeliverSmError::BufferTooShort {
            expected: 20,
            actual: buffer.len(),
        });
    }
    
    let originator_start = 19;
    let originator_end = buffer[originator_start..]
        .iter()
        .position(|&b| b == 0)
        .map(|pos| originator_start + pos)
        .ok_or(DeliverSmError::NullTerminatorNotFound)?;
    
    let originator_address = String::from_utf8(buffer[originator_start..originator_end].to_vec())
        .map_err(|_| DeliverSmError::InvalidOriginatorAddress)?;
    
    // Find destination address (starts after originator + 3 bytes for TON/NPI fields)
    let dest_start = originator_end + 3;
    if buffer.len() <= dest_start {
        return Err(DeliverSmError::BufferTooShort {
            expected: dest_start + 1,
            actual: buffer.len(),
        });
    }
    
    let dest_end = buffer[dest_start..]
        .iter()
        .position(|&b| b == 0)
        .map(|pos| dest_start + pos)
        .ok_or(DeliverSmError::NullTerminatorNotFound)?;
    
    let destination_address = String::from_utf8(buffer[dest_start..dest_end].to_vec())
        .map_err(|_| DeliverSmError::InvalidDestinationAddress)?;
    
    // Get message length (at dest_end + 10 bytes for various fields)
    let text_length_pos = dest_end + 10;
    if buffer.len() <= text_length_pos {
        return Err(DeliverSmError::BufferTooShort {
            expected: text_length_pos + 1,
            actual: buffer.len(),
        });
    }
    
    let text_length = buffer[text_length_pos];
    
    // Extract message content
    let message_start = text_length_pos + 1;
    let message_end = message_start + text_length as usize;
    
    if buffer.len() < message_end {
        return Err(DeliverSmError::BufferTooShort {
            expected: message_end,
            actual: buffer.len(),
        });
    }
    
    let message = String::from_utf8(buffer[message_start..message_end].to_vec())
        .map_err(|_| DeliverSmError::InvalidMessageContent)?;

    let returned_msg_id: u64 = MSG_ID.captures(&message)
    .and_then(|caps| caps.get(1))
    .and_then(|m| {
        m.as_str()
        .split(':')
        .nth(1)
        .and_then(|s| s.trim().parse::<u64>().ok())
    }).ok_or(DeliverSmError::InvalidMessageContent)?;
    
    let returned_msg_status = MSG_STATUS.captures(&message)
    .and_then(|caps| caps.get(1))
    .map(|m| {
        m.as_str()
        .split_once(':')
        .map(|(_,second)| second)
        .unwrap_or(m.as_str())
        .to_string()
    }).ok_or(DeliverSmError::InvalidMessageContent)?;
    
    Ok(DeliverSm {
        sequence_number,
        originator_address,
        destination_address,
        message,
        returned_msg_id,
        returned_msg_status,
    })
}

/// Creates a deliver_sm_resp PDU asynchronously
/// 
/// This function generates a success response PDU for a deliver_sm request.
/// 
/// # Arguments
/// * `sequence_number` - The sequence number from the original deliver_sm PDU
/// 
/// # Returns
/// * `Ok(Vec<u8>)` - The binary response PDU ready to be sent
/// * `Err(DeliverSmError)` - If an error occurs during PDU creation
/// 
/// # Example
/// ```rust
/// let response = create_deliver_sm_resp_async(12345).await?;
/// // Send response over network connection
/// ```
pub async fn create_deliver_sm_resp_async(sequence_number: u32) -> DeliverSmResult<Vec<u8>> {
    let command_length = 17u32; // Fixed size for deliver_sm_resp
    let mut pdu = Vec::with_capacity(command_length as usize);
    
    // Write PDU header
    pdu.write_all(&command_length.to_be_bytes())?;              // Command Length
    pdu.write_all(&command_id::DELIVER_SM_RESP.to_be_bytes())?;     // Command ID
    pdu.write_all(&0x00000000u32.to_be_bytes())?;            // Command Status (success)
    pdu.write_all(&sequence_number.to_be_bytes())?;            // Sequence Number
    
    // Write empty message ID (null-terminated)
    pdu.write_all(&[0u8])?;
    
    Ok(pdu)
}

/// Creates a generic_nack PDU asynchronously
/// 
/// This function generates a generic NACK (negative acknowledgment) PDU
/// used when a received PDU could not be processed.
/// 
/// # Arguments
/// * `sequence_number` - The sequence number from the original PDU (0 if unknown)
/// 
/// # Returns
/// * `Ok(Vec<u8>)` - The binary generic_nack PDU ready to be sent
/// * `Err(DeliverSmError)` - If an error occurs during PDU creation
/// 
/// # Example
/// ```rust
/// let nack = create_generic_nack_async(12345).await?;
/// // Send NACK over network connection
/// ```
pub async fn create_generic_nack_async(sequence_number: u32) -> DeliverSmResult<Vec<u8>> {
    let command_length = 16u32; // Fixed size for generic_nack
    let mut pdu = Vec::with_capacity(command_length as usize);
    
    // Write PDU header
    pdu.write_all(&command_length.to_be_bytes())?;              // Command Length
    pdu.write_all(&command_id::GENERIC_NACK.to_be_bytes())?;        // Command ID
    pdu.write_all(&0x00000001u32.to_be_bytes())?;     // Command Status (invalid message)
    pdu.write_all(&sequence_number.to_be_bytes())?;            // Sequence Number
    
    Ok(pdu)
}

/// Result of parsing a Deliver message, containing originator and destination address
/// and response and message.
/// 
/// This structure combines the response bytes and the originator and destination address received.
/// The originator is extracted from Originator Address and destination from Destination Address 
/// and message received following SMPPv5 documentation.
#[derive(Debug)]
pub struct DeliverSMParsedResult {
    /// SIP Response to the parsed message
    pub deliversm_resp_bytes: Vec<u8>,
    /// Originator Address of the Message
    pub orig_addr: Option<String>,
    /// Destination Address of the Message
    pub dest_addr: Option<String>,
    /// Message received in response
    pub message: Option<String>,
    /// Message ID of SubmitSM received in response
    pub msg_id: Option<String>,
    /// Message Status of SubmitSM received in response
    pub msg_status: Option<String>,
}

/// Main deliver_sm processing function (equivalent to Python function)
/// 
/// This function processes a deliver_sm buffer and returns the appropriate response.
/// It matches the behavior of the original Python function.
/// 
/// # Arguments
/// * `buffer` - The raw PDU buffer received from the network
/// 
/// # Returns
/// * `Result<DeliverSMParsedResult, &'static str>` - Response PDU (either deliver_sm_resp or generic_nack
///                                                           along with orig and dest addr if present)
/// 
/// # Example
/// ```rust
/// let buffer = vec![/* PDU bytes */];
/// let response = deliver_sm_async(&buffer).await;
/// // Send response over network connection
/// ```
pub async fn deliver_sm_async(buffer: &[u8]) -> Result<DeliverSMParsedResult, &'static str> {
    let (deliversm_resp_bytes, orig_addr, dest_addr, message, returned_msg_id, msg_status) = match parse_deliver_sm_async(buffer).await {
        Ok(deliver_sm) => {
            // Successfully parsed - log information and create success response
            println!("DeliverSM Received");
            println!(
                "Originator Address: {},
                Destination Address: {},
                Message: {},
                Message ID: {},
                Message Status: {}", 
                deliver_sm.originator_address, 
                deliver_sm.destination_address, 
                deliver_sm.message,
                deliver_sm.returned_msg_id,
                deliver_sm.returned_msg_status
            );
            
            // Create success response
            match create_deliver_sm_resp_async(deliver_sm.sequence_number).await {
                Ok(response) => (response,
                    Some(deliver_sm.originator_address),
                    Some(deliver_sm.destination_address),
                    Some(deliver_sm.message),
                    Some(deliver_sm.returned_msg_id.to_string()),
                    Some(deliver_sm.returned_msg_status)),
                Err(_) => {
                    // Fallback to generic NACK if we can't create response
                    (create_generic_nack_async(0).await.unwrap_or_else(|_| vec![0u8; 16]),None,None,None,None,None)
                }
            }
        }
        Err(_) => {
            // Parsing failed - send generic NACK
            println!("DeliverSM could not be parsed. Sending generic Nack");
            (create_generic_nack_async(0).await.unwrap_or_else(|_| vec![0u8; 16]),None,None,None,None,None)
        }
    };
    Ok(DeliverSMParsedResult {
        deliversm_resp_bytes,
        orig_addr,
        dest_addr,
        message,
        msg_id: returned_msg_id,
        msg_status,
    })
}


