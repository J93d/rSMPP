use rand::Rng;

use crate::common::command_id;

pub struct EnquireLink;

impl EnquireLink {
    pub fn create_pdu() -> Vec<u8> {
        let mut rng = rand::thread_rng();
        let sequence_number: u32 = rng.gen_range(1..=65535);
        
        // Fixed header part: Length (16), Cmd ID (EnquireLink), Status (0)
        let mut pdu = vec![
            0x00, 0x00, 0x00, 0x10, // Command Length = 16
        ];
        pdu.extend_from_slice(&command_id::ENQUIRE_LINK.to_be_bytes()); // Command ID
        pdu.extend_from_slice(&0u32.to_be_bytes()); // Command Status = 0

        
        pdu.extend_from_slice(&sequence_number.to_be_bytes());
        pdu
    }
}
