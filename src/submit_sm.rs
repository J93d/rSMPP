use std::io::{Error, ErrorKind, Result};
use rand::Rng;
use crate::common::{self, command_id};
use crate::gsm_encoding;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Encoding {
    Gsm7Bit,
    Latin1,
    Ucs2,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MultipartMode {
    Udh,
    Sar,
    Payload,
}

#[derive(Debug, Clone)]
pub struct Tlv {
    pub tag: u16,
    pub length: u16,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub enum TypeOfNumber {
    Unknown = 0,
    International = 1,
    National = 2,
    NetworkSpecific = 3,
    SubscriberNumber = 4,
    Alphanumeric = 5,
    Abbreviated = 6,
}

impl From<u8> for TypeOfNumber {
    fn from(value: u8) -> Self {
        match value {
            1 => TypeOfNumber::International,
            2 => TypeOfNumber::National,
            3 => TypeOfNumber::NetworkSpecific,
            4 => TypeOfNumber::SubscriberNumber,
            5 => TypeOfNumber::Alphanumeric,
            6 => TypeOfNumber::Abbreviated,
            _ => TypeOfNumber::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NumericPlanIndicator {
    Unknown = 0,
    ISDN = 1,
    Data = 3,
    Telex = 4,
    LandMobile = 6,
    National = 8,
    Private = 9,
    ERMES = 10,
    Internet = 14,
    WAPClientId = 18,
}

impl From<u8> for NumericPlanIndicator {
    fn from(value: u8) -> Self {
        match value {
            1 => NumericPlanIndicator::ISDN,
            3 => NumericPlanIndicator::Data,
            4 => NumericPlanIndicator::Telex,
            6 => NumericPlanIndicator::LandMobile,
            8 => NumericPlanIndicator::National,
            9 => NumericPlanIndicator::Private,
            10 => NumericPlanIndicator::ERMES,
            14 => NumericPlanIndicator::Internet,
            18 => NumericPlanIndicator::WAPClientId,
            _ => NumericPlanIndicator::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubmitSmResponse {
    pub command_status: u32,
    pub status_name: String,
    pub sequence_number: u32,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SubmitSmParams {
    pub source_ton: TypeOfNumber,
    pub source_npi: NumericPlanIndicator,
    pub source_addr: String,
    pub dest_ton: TypeOfNumber,
    pub dest_npi: NumericPlanIndicator,
    pub destination_addr: String,
    pub esm_class: u8,
    pub protocol_id: u8,
    pub schedule_delivery_time: String,
    pub validity_period: String,
    pub registered_delivery: u8,
    pub data_coding: u8,
    pub message: Vec<u8>,
    pub optional_params: Vec<Tlv>,
}

impl Default for SubmitSmParams {
    fn default() -> Self {
        Self {
            source_ton: TypeOfNumber::Unknown,
            source_npi: NumericPlanIndicator::Unknown,
            source_addr: String::new(),
            dest_ton: TypeOfNumber::Unknown,
            dest_npi: NumericPlanIndicator::Unknown,
            destination_addr: String::new(),
            esm_class: 0,
            protocol_id: 0,
            schedule_delivery_time: String::new(),
            validity_period: String::new(),
            registered_delivery: 0,
            data_coding: 0,
            message: Vec::new(),
            optional_params: Vec::new(),
        }
    }
}

pub struct SubmitSmBuilder {
    params: SubmitSmParams,
}

impl SubmitSmBuilder {
    pub fn new() -> Self {
        Self {
            params: SubmitSmParams::default(),
        }
    }

    pub fn source(mut self, ton: TypeOfNumber, npi: NumericPlanIndicator, addr: String) -> Self {
        self.params.source_ton = ton;
        self.params.source_npi = npi;
        self.params.source_addr = addr;
        self
    }

    pub fn destination(mut self, ton: TypeOfNumber, npi: NumericPlanIndicator, addr: String) -> Self {
        self.params.dest_ton = ton;
        self.params.dest_npi = npi;
        self.params.destination_addr = addr;
        self
    }

    pub fn message(mut self, msg: Vec<u8>) -> Self {
        self.params.message = msg;
        self
    }

    pub fn esm_class(mut self, class: u8) -> Self {
        self.params.esm_class = class;
        self
    }
    
    pub fn data_coding(mut self, coding: u8) -> Self {
        self.params.data_coding = coding;
        self
    }
    
    pub fn add_tlv(mut self, tag: u16, value: Vec<u8>) -> Self {
        self.params.optional_params.push(Tlv {
            tag,
            length: value.len() as u16,
            value,
        });
        self
    }

    pub fn build(self) -> SubmitSmParams {
        self.params
    }
}

pub struct SubmitSm;

impl SubmitSm {
    pub async fn create_pdus(
        source: String, src_ton: u8, src_npi: u8,
        dest: String, dest_ton: u8, dest_npi: u8,
        text: String, 
        encoding: Encoding, 
        mode: MultipartMode,
        pid: u8, dcs_override: Option<u8>, validity: String, dlr: bool
    ) -> Result<Vec<Vec<u8>>> {
        let (encoded_bytes, default_data_coding) = match encoding {
            Encoding::Gsm7Bit => (gsm_encoding::gsm_7bit_encode(&text).map_err(|e| Error::new(ErrorKind::InvalidInput, e))?, 0x00),
            Encoding::Latin1 => (gsm_encoding::encode_8bit(&text), 0x03),
            Encoding::Ucs2 => (gsm_encoding::encode_16bit(&text), 0x08),
        };

        let data_coding = dcs_override.unwrap_or(default_data_coding);
        let registered_delivery = if dlr { 1 } else { 0 };

        // Determine max segment length
        let (single_max, multipart_max) = match encoding {
            Encoding::Gsm7Bit => (160, 153), // 160 chars vs 153 chars (approx 7 bytes UDH)
            Encoding::Latin1 => (140, 134),  // 140 bytes vs 134 bytes
            Encoding::Ucs2 => (140, 134),    // 140 bytes vs 134 bytes
        };

        if encoded_bytes.len() <= single_max || mode == MultipartMode::Payload {
            // Single message or Payload mode
            let mut params = SubmitSmParams::default();
            params.source_addr = source;
            params.source_ton = TypeOfNumber::from(src_ton);
            params.source_npi = NumericPlanIndicator::from(src_npi);
            params.destination_addr = dest;
            params.dest_ton = TypeOfNumber::from(dest_ton);
            params.dest_npi = NumericPlanIndicator::from(dest_npi);
            params.data_coding = data_coding;
            params.protocol_id = pid;
            params.validity_period = validity.clone();
            params.registered_delivery = registered_delivery;

            if mode == MultipartMode::Payload && encoded_bytes.len() > single_max {
                // Use Message Payload TLV (0x0424)
                params.optional_params.push(Tlv {
                    tag: 0x0424,
                    length: encoded_bytes.len() as u16,
                    value: encoded_bytes,
                });
                params.message = Vec::new(); // Empty short_message field
            } else {
                params.message = encoded_bytes;
            }
            
            return Ok(vec![Self::create_pdu(params)?]);
        }

        // Split message
        let mut pdus = Vec::new();
        let ref_num = rand::thread_rng().gen_range(1..255) as u8;
        let total_segments = ((encoded_bytes.len() as f64) / (multipart_max as f64)).ceil() as u8;

        for seq_num in 1..=total_segments {
            let start = ((seq_num - 1) as usize) * multipart_max;
            let end = std::cmp::min(start + multipart_max, encoded_bytes.len());
            let segment_data = encoded_bytes[start..end].to_vec();

            let mut params = SubmitSmParams::default();
            params.source_addr = source.clone();
            params.source_ton = TypeOfNumber::from(src_ton);
            params.source_npi = NumericPlanIndicator::from(src_npi);
            params.destination_addr = dest.clone();
            params.dest_ton = TypeOfNumber::from(dest_ton);
            params.dest_npi = NumericPlanIndicator::from(dest_npi);
            params.data_coding = data_coding;
            params.protocol_id = pid;
            params.validity_period = validity.clone();
            params.registered_delivery = registered_delivery;

            match mode {
                MultipartMode::Udh => {
                    params.esm_class = 0x40; // UDHI flag set
                    let mut udh = vec![0x05, 0x00, 0x03, ref_num, total_segments, seq_num];
                    udh.extend(segment_data);
                    params.message = udh;
                },
                MultipartMode::Sar => {
                    params.message = segment_data;
                    // SAR TLVs
                    params.optional_params.push(Tlv { tag: 0x020C, length: 2, value: vec![0, ref_num] }); // sar_msg_ref_num
                    params.optional_params.push(Tlv { tag: 0x020E, length: 1, value: vec![total_segments] }); // sar_total_segments
                    params.optional_params.push(Tlv { tag: 0x020F, length: 1, value: vec![seq_num] }); // sar_segment_seqnum
                },
                MultipartMode::Payload => unreachable!("Payload handled in single message block"),
            }

            pdus.push(Self::create_pdu(params)?);
        }

        Ok(pdus)
    }

    pub fn create_pdu(params: SubmitSmParams) -> Result<Vec<u8>> {
        Self::validate_params(&params)?;

        let mut rng = rand::thread_rng();
        let sequence_number: u32 = rng.gen_range(1..=65535);

        let service_type = b"";
        let source_addr = params.source_addr.as_bytes();
        let destination_addr = params.destination_addr.as_bytes();
        let schedule_delivery_time = params.schedule_delivery_time.as_bytes();
        let validity_period = params.validity_period.as_bytes();
        let message_bytes = &params.message;

        let fixed_length = 33;
        let mut command_length = fixed_length 
            + service_type.len()
            + source_addr.len() 
            + destination_addr.len()
            + schedule_delivery_time.len()
            + validity_period.len()
            + message_bytes.len();
        
        for tlv in &params.optional_params {
            command_length += 4 + tlv.value.len(); // Tag(2) + Length(2) + Value
        }

        let mut pdu = Vec::with_capacity(command_length);

        pdu.extend_from_slice(&(command_length as u32).to_be_bytes());
        pdu.extend_from_slice(&command_id::SUBMIT_SM.to_be_bytes());
        pdu.extend_from_slice(&0u32.to_be_bytes());
        pdu.extend_from_slice(&sequence_number.to_be_bytes());

        pdu.extend_from_slice(service_type);
        pdu.push(0);

        pdu.push(params.source_ton as u8);
        pdu.push(params.source_npi as u8);
        pdu.extend_from_slice(source_addr);
        pdu.push(0);

        pdu.push(params.dest_ton as u8);
        pdu.push(params.dest_npi as u8);
        pdu.extend_from_slice(destination_addr);
        pdu.push(0);

        pdu.push(params.esm_class);
        pdu.push(params.protocol_id);
        pdu.push(0);

        pdu.extend_from_slice(schedule_delivery_time);
        pdu.push(0);

        pdu.extend_from_slice(validity_period);
        pdu.push(0);

        pdu.push(params.registered_delivery);
        pdu.push(0);

        pdu.push(params.data_coding);
        pdu.push(0);

        pdu.push(message_bytes.len() as u8);
        pdu.extend_from_slice(message_bytes);

        // Add Optional Parameters
        for tlv in &params.optional_params {
            pdu.extend_from_slice(&tlv.tag.to_be_bytes());
            pdu.extend_from_slice(&tlv.length.to_be_bytes());
            pdu.extend_from_slice(&tlv.value);
        }

        Ok(pdu)
    }

    fn validate_params(params: &SubmitSmParams) -> Result<()> {
        if params.message.len() > 255 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Message length exceeds 255 bytes limit for short message",
            ));
        }

        if params.source_addr.len() > 21 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Source address exceeds 21 character limit",
            ));
        }

        if params.destination_addr.len() > 21 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Destination address exceeds 21 character limit",
            ));
        }

        Ok(())
    }

    pub async fn parse_submit_sm_resp(buffer: &[u8]) -> Result<SubmitSmResponse> {
        if buffer.len() < 16 {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Buffer too short",
            ));
        }

        let status_bytes = [buffer[8], buffer[9], buffer[10], buffer[11]];
        let command_status = u32::from_be_bytes(status_bytes);

        let seq_bytes = [buffer[12], buffer[13], buffer[14], buffer[15]];
        let sequence_number = u32::from_be_bytes(seq_bytes);

        let status_name = common::get_status_description(command_status);

        let message_id = if command_status == 0 && buffer.len() > 16 {
            let s = String::from_utf8_lossy(&buffer[16..].iter().take_while(|&&b| b != 0).cloned().collect::<Vec<u8>>()).to_string();
            Some(s)
        } else {
            None
        };

        Ok(SubmitSmResponse {
            command_status,
            status_name,
            sequence_number,
            message_id,
        })
    }
}
