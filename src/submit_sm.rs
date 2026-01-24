use std::io::{Error, ErrorKind, Result};
use rand::Rng;
use crate::smpp_error_codes;

const SUBMIT_SM: u32 = 0x00000004;

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
    pub message: String,
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
            message: String::new(),
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

    pub fn message(mut self, msg: String) -> Self {
        self.params.message = msg;
        self
    }

    pub fn esm_class(mut self, class: u8) -> Self {
        self.params.esm_class = class;
        self
    }

    pub fn protocol_id(mut self, pid: u8) -> Self {
        self.params.protocol_id = pid;
        self
    }

    pub fn schedule_delivery_time(mut self, time: String) -> Self {
        self.params.schedule_delivery_time = time;
        self
    }

    pub fn validity_period(mut self, period: String) -> Self {
        self.params.validity_period = period;
        self
    }

    pub fn registered_delivery(mut self, flags: u8) -> Self {
        self.params.registered_delivery = flags;
        self
    }

    pub fn data_coding(mut self, coding: u8) -> Self {
        self.params.data_coding = coding;
        self
    }

    pub fn build(self) -> SubmitSmParams {
        self.params
    }
}

impl Default for SubmitSmBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SubmitSm;

impl SubmitSm {
    pub async fn create_pdu(params: SubmitSmParams) -> Result<Vec<u8>> {
        Self::validate_params(&params)?;

        let mut rng = rand::thread_rng();
        let sequence_number: u32 = rng.gen_range(1..=65535);

        let service_type = b"";
        let source_addr = params.source_addr.as_bytes();
        let destination_addr = params.destination_addr.as_bytes();
        let schedule_delivery_time = params.schedule_delivery_time.as_bytes();
        let validity_period = params.validity_period.as_bytes();
        let message_bytes = params.message.as_bytes();

        let fixed_length = 33;
        let command_length = fixed_length 
            + service_type.len()
            + source_addr.len() 
            + destination_addr.len()
            + schedule_delivery_time.len()
            + validity_period.len()
            + message_bytes.len();

        let mut pdu = Vec::with_capacity(command_length);

        pdu.extend_from_slice(&(command_length as u32).to_be_bytes());
        pdu.extend_from_slice(&SUBMIT_SM.to_be_bytes());
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
        Ok(pdu)
    }

    fn validate_params(params: &SubmitSmParams) -> Result<()> {


        if params.message.as_bytes().len() > 255 {
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

        if !params.schedule_delivery_time.is_empty() && params.schedule_delivery_time.len() != 16 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Schedule delivery time must be empty or 16 characters (YYMMDDhhmmsstnn)",
            ));
        }

        if !params.validity_period.is_empty() && params.validity_period.len() != 16 {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Validity period must be empty or 16 characters (YYMMDDhhmmsstnn)",
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

        let status_name = smpp_error_codes::error_codes(command_status);

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
