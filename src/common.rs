/// SMPP Interface Version 3.4
pub const SMPP_INTERFACE_VERSION: u8 = 0x34;

/// Address Type of Number (TON)
pub mod ton {
    pub const UNKNOWN: u8 = 0x00;
    pub const INTERNATIONAL: u8 = 0x01;
    pub const NATIONAL: u8 = 0x02;
    pub const NETWORK_SPECIFIC: u8 = 0x03;
    pub const SUBSCRIBER_NUMBER: u8 = 0x04;
    pub const ALPHANUMERIC: u8 = 0x05;
    pub const ABBREVIATED: u8 = 0x06;
}

/// Address Numbering Plan Indicator (NPI)
pub mod npi {
    pub const UNKNOWN: u8 = 0x00;
    pub const ISDN: u8 = 0x01;
    pub const DATA: u8 = 0x03;
    pub const TELEX: u8 = 0x04;
    pub const LAND_MOBILE: u8 = 0x06;
    pub const NATIONAL: u8 = 0x08;
    pub const PRIVATE: u8 = 0x09;
    pub const ERMES: u8 = 0x0A;
    pub const INTERNET: u8 = 0x0E;
    pub const WAP: u8 = 0x12;
}

/// Command IDs
pub mod command_id {
    pub const GENERIC_NACK: u32 = 0x80000000;
    pub const BIND_RECEIVER: u32 = 0x00000001;
    pub const BIND_RECEIVER_RESP: u32 = 0x80000001;
    pub const BIND_TRANSMITTER: u32 = 0x00000002;
    pub const BIND_TRANSMITTER_RESP: u32 = 0x80000002;
    pub const SUBMIT_SM: u32 = 0x00000004;
    pub const SUBMIT_SM_RESP: u32 = 0x80000004;
    pub const DELIVER_SM: u32 = 0x00000005;
    pub const DELIVER_SM_RESP: u32 = 0x80000005;
    pub const BIND_TRANSCEIVER: u32 = 0x00000009;
    pub const BIND_TRANSCEIVER_RESP: u32 = 0x80000009;
    pub const ENQUIRE_LINK: u32 = 0x00000015;
    pub const ENQUIRE_LINK_RESP: u32 = 0x80000015;
}

/// Command Status
pub const COMMAND_STATUS_OK: u32 = 0x00000000;

/// Helper function to get error description from status code
pub fn get_status_description(status: u32) -> String {
    match status {
        0x00000000 => "ESME_ROK".to_string(),
        0x00000001 => "ESME_RINVMSGLEN".to_string(),
        0x00000002 => "ESME_RINVCMDLEN".to_string(),
        0x00000003 => "ESME_RINVCMDID".to_string(),
        0x00000004 => "ESME_RINVBNDSTS".to_string(),
        0x00000005 => "ESME_RALYBND".to_string(),
        0x00000006 => "ESME_RINVPRTFLG".to_string(),
        0x00000007 => "ESME_RINVREGDLVFLG".to_string(),
        0x00000008 => "ESME_RSYSERR".to_string(),
        0x0000000A => "ESME_RINVSRCADR".to_string(),
        0x0000000B => "ESME_RINVDSTADR".to_string(),
        0x0000000C => "ESME_RINVMSGID".to_string(),
        0x0000000D => "ESME_RBINDFAIL".to_string(),
        0x0000000E => "ESME_RINVPASWD".to_string(),
        0x0000000F => "ESME_RINVSYSID".to_string(),
        0x00000011 => "ESME_RCANCELFAIL".to_string(),
        0x00000013 => "ESME_RREPLACEFAIL".to_string(),
        0x00000014 => "ESME_RMSGQFUL".to_string(),
        0x00000015 => "ESME_RINVSERVICETYPE".to_string(),
        0x00000033 => "ESME_RINVNUMDESTS".to_string(),
        0x00000034 => "ESME_RINVDLNAME".to_string(),
        0x00000040 => "ESME_RINVDESTFLAG".to_string(),
        0x00000042 => "ESME_RINVSUBREP".to_string(),
        0x00000043 => "ESME_RINVESMCLASS".to_string(),
        0x00000044 => "ESME_RCNTSUBDL".to_string(),
        0x00000045 => "ESME_RSUBMITFAIL".to_string(),
        0x00000048 => "ESME_RINVSRCTON".to_string(),
        0x00000049 => "ESME_RINVSRCNPI".to_string(),
        0x00000050 => "ESME_RINVDSTTON".to_string(),
        0x00000051 => "ESME_RINVDSTNPI".to_string(),
        0x00000053 => "ESME_RINVSYSTYP".to_string(),
        0x00000054 => "ESME_RINVREPFLAG".to_string(),
        0x00000055 => "ESME_RINVNUMMSGS".to_string(),
        0x00000058 => "ESME_RTHROTTLED".to_string(),
        0x00000061 => "ESME_RINVSCHED".to_string(),
        0x00000062 => "ESME_RINVEXPIRY".to_string(),
        0x00000063 => "ESME_RINVDFTMSGID".to_string(),
        0x00000064 => "ESME_RX_T_APPN".to_string(),
        0x00000065 => "ESME_RX_P_APPN".to_string(),
        0x00000066 => "ESME_RX_R_APPN".to_string(),
        0x00000067 => "ESME_RQUERYFAIL".to_string(),
        0x000000C0 => "ESME_RINVOPTPARSTREAM".to_string(),
        0x000000C1 => "ESME_ROPTPARNOTALLWD".to_string(),
        0x000000C2 => "ESME_RINVPARLEN".to_string(),
        0x000000C3 => "ESME_RMISSINGOPTPARAM".to_string(),
        0x000000C4 => "ESME_RINVOPTPARAMVAL".to_string(),
        0x000000FE => "ESME_RDELIVERYFAILURE".to_string(),
        0x000000FF => "ESME_RUNKNOWNERR".to_string(),
        _ => format!("Unknown Error: 0x{:08X}", status),
    }
}
