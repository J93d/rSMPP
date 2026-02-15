use rand::Rng;
use smpp_codec::common::{BindMode, Npi, Ton};
use smpp_codec::pdus::{
    BindRequest, CancelSmRequest, Destination, QuerySmRequest, ReplaceSm, SubmitMulti,
    SubmitSmRequest, UnbindRequest,
};
use smpp_codec::splitter::{EncodingType, MessageSplitter, SplitMode};
use smpp_codec::tlv::{Tlv, tags};

pub struct PduFactory;

impl PduFactory {
    pub fn create_bind_request(
        seq_num: u32,
        bind_mode: &str,
        system_id: &str,
        password: &str,
    ) -> Vec<u8> {
        let mode_enum = match bind_mode {
            "Transmitter" => BindMode::Transmitter,
            "Receiver" => BindMode::Receiver,
            "Transceiver" => BindMode::Transceiver,
            _ => BindMode::Transceiver,
        };

        let req = BindRequest::new(
            seq_num,
            mode_enum,
            system_id.to_string(),
            password.to_string(),
        );
        let mut pdu = Vec::new();
        req.encode(&mut pdu).expect("Failed to encode BindRequest");
        pdu
    }

    pub fn create_unbind_request(seq_num: u32) -> Vec<u8> {
        let req = UnbindRequest::new(seq_num);
        let mut pdu = Vec::new();
        req.encode(&mut pdu)
            .expect("Failed to encode UnbindRequest");
        pdu
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_submit_pdus(
        mut seq_num: u32,
        source: &str,
        src_ton: &str,
        src_npi: &str,
        dest: &str,
        dest_ton: &str,
        dest_npi: &str,
        message: &str,
        encoding: &str,
        mode: &str,
        pid: &str,
        dcs: &str,
        validity: &str,
        dlr: bool,
    ) -> Result<Vec<Vec<u8>>, String> {
        let enc_enum = match encoding {
            "GSM 7-bit" => EncodingType::Gsm7Bit,
            "Latin-1" => EncodingType::Latin1,
            "UCS-2" => EncodingType::Ucs2,
            _ => EncodingType::Gsm7Bit,
        };

        let mode_enum = match mode {
            "UDH" => SplitMode::Udh,
            "SAR" => SplitMode::Sar,
            "Payload" => SplitMode::Payload,
            _ => SplitMode::Udh,
        };

        let (parts, data_coding_auto) =
            MessageSplitter::split(message.to_string(), enc_enum, mode_enum)
                .map_err(|e| format!("Split error: {}", e))?;

        let total = parts.len();
        let sar_ref_num = rand::thread_rng().r#gen::<u16>();
        let mut pdus = Vec::new();

        for (i, part) in parts.into_iter().enumerate() {
            // Parse destinations to check if it's a multi-submit
            let dests: Vec<&str> = dest
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            let mut pdu_bytes = Vec::new();

            if dests.len() > 1 {
                // SubmitMulti Logic
                let destinations: Vec<Destination> = dests
                    .iter()
                    .map(|d| Destination::SmeAddress {
                        ton: Ton::from(dest_ton.parse::<u8>().unwrap_or(0)),
                        npi: Npi::from(dest_npi.parse::<u8>().unwrap_or(0)),
                        address: d.to_string(),
                    })
                    .collect();

                let mut req = SubmitMulti::new(
                    seq_num,
                    source.to_string(),
                    destinations,
                    if mode_enum == SplitMode::Payload {
                        Vec::new()
                    } else {
                        part.clone()
                    },
                );

                if mode_enum == SplitMode::Payload {
                    req.optional_params
                        .push(Tlv::new(tags::MESSAGE_PAYLOAD, part));
                }

                // Set Common Fields
                req.data_coding = dcs.parse().unwrap_or(data_coding_auto);
                if let Ok(pid_val) = pid.parse() {
                    req.protocol_id = pid_val;
                }
                req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
                req.validity_period = validity.to_string();
                req.registered_delivery = if dlr { 1 } else { 0 };

                if mode_enum == SplitMode::Udh && total > 1 {
                    req.esm_class = 0x40; // UDHI
                }

                if mode_enum == SplitMode::Sar && total > 1 {
                    req.optional_params
                        .push(Tlv::new_u16(tags::SAR_MSG_REF_NUM, sar_ref_num));
                    req.optional_params
                        .push(Tlv::new_u8(tags::SAR_TOTAL_SEGMENTS, total as u8));
                    req.optional_params
                        .push(Tlv::new_u8(tags::SAR_SEGMENT_SEQNUM, (i + 1) as u8));
                }

                req.encode(&mut pdu_bytes)
                    .map_err(|e| format!("Encode error (Multi): {:?}", e))?;
            } else {
                // SubmitSm Logic (Existing)
                let mut req = if mode_enum == SplitMode::Payload {
                    // For Payload mode, the short_message field is empty,
                    // and the content goes into the message_payload TLV.
                    let mut r = SubmitSmRequest::new(
                        seq_num,
                        source.to_string(),
                        dest.to_string(),
                        Vec::new(),
                    );
                    r.add_tlv(Tlv::new(tags::MESSAGE_PAYLOAD, part));
                    r
                } else {
                    SubmitSmRequest::new(seq_num, source.to_string(), dest.to_string(), part)
                };

                // Set fields
                req.data_coding = dcs.parse().unwrap_or(data_coding_auto);
                if let Ok(pid_val) = pid.parse() {
                    req.protocol_id = pid_val;
                }
                // Type of Number & NPI mapping
                req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
                req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
                req.dest_addr_ton = Ton::from(dest_ton.parse::<u8>().unwrap_or(0));
                req.dest_addr_npi = Npi::from(dest_npi.parse::<u8>().unwrap_or(0));

                // Validity
                req.validity_period = validity.to_string();
                req.registered_delivery = if dlr { 1 } else { 0 };

                if mode_enum == SplitMode::Udh && total > 1 {
                    req.esm_class = 0x40; // UDHI
                }

                if mode_enum == SplitMode::Sar && total > 1 {
                    // SAR Mode: Add SAR TLVs for reconstruction at the receiver
                    req.add_tlv(Tlv::new_u16(tags::SAR_MSG_REF_NUM, sar_ref_num));
                    req.add_tlv(Tlv::new_u8(tags::SAR_TOTAL_SEGMENTS, total as u8));
                    req.add_tlv(Tlv::new_u8(tags::SAR_SEGMENT_SEQNUM, (i + 1) as u8));
                }

                req.encode(&mut pdu_bytes)
                    .map_err(|e| format!("Encode error: {:?}", e))?;
            }

            pdus.push(pdu_bytes);
            seq_num += 1;
        }

        Ok(pdus)
    }

    pub fn create_query_sm_request(
        seq_num: u32,
        msg_id: &str,
        source: &str,
        ton: &str,
        npi: &str,
    ) -> Vec<u8> {
        let mut req = QuerySmRequest::new(seq_num, msg_id.to_string(), source.to_string());
        req.source_addr_ton = Ton::from(ton.parse::<u8>().unwrap_or(0));
        req.source_addr_npi = Npi::from(npi.parse::<u8>().unwrap_or(0));

        let mut pdu = Vec::new();
        req.encode(&mut pdu)
            .expect("Failed to encode QuerySmRequest");
        pdu
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_cancel_sm_request(
        seq_num: u32,
        msg_id: &str,
        source: &str,
        src_ton: &str,
        src_npi: &str,
        dest: &str,
        dest_ton: &str,
        dest_npi: &str,
    ) -> Vec<u8> {
        let mut req = CancelSmRequest::new(
            seq_num,
            msg_id.to_string(),
            source.to_string(),
            dest.to_string(),
        );
        req.service_type = "SMPP".to_string();
        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));
        req.dest_addr_ton = Ton::from(dest_ton.parse::<u8>().unwrap_or(0));
        req.dest_addr_npi = Npi::from(dest_npi.parse::<u8>().unwrap_or(0));

        let mut pdu = Vec::new();
        req.encode(&mut pdu)
            .expect("Failed to encode CancelSmRequest");
        pdu
    }

    pub fn create_replace_sm_request(
        seq_num: u32,
        msg_id: &str,
        source: &str,
        src_ton: &str,
        src_npi: &str,
        message: &str,
    ) -> Vec<u8> {
        let mut req = ReplaceSm::new(
            seq_num,
            msg_id.to_string(),
            source.to_string(),
            message.to_string().into_bytes(),
        );
        req.source_addr_ton = Ton::from(src_ton.parse::<u8>().unwrap_or(0));
        req.source_addr_npi = Npi::from(src_npi.parse::<u8>().unwrap_or(0));

        let mut pdu = Vec::new();
        req.encode(&mut pdu).expect("Failed to encode ReplaceSm");
        pdu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_request_creation() {
        let pdu = PduFactory::create_bind_request(1, "Transmitter", "sys", "pwd");
        assert!(!pdu.is_empty());
        // Simple length check or more complex decode if needed
        assert!(pdu.len() > 16);
    }

    #[test]
    fn test_submit_sm_creation() {
        let pdus = PduFactory::create_submit_pdus(
            1,
            "src",
            "0",
            "0",
            "dest",
            "0",
            "0",
            "Hello",
            "GSM 7-bit",
            "UDH",
            "",
            "",
            "",
            false,
        )
        .expect("Failed to create submit pdus");

        assert_eq!(pdus.len(), 1);
        assert!(pdus[0].len() > 16);
    }

    #[test]
    fn test_submit_multi_creation() {
        let pdus = PduFactory::create_submit_pdus(
            1,
            "src",
            "0",
            "0",
            "dest1,dest2",
            "0",
            "0",
            "Multi",
            "GSM 7-bit",
            "UDH",
            "",
            "",
            "",
            false,
        )
        .expect("Failed to create submit multi pdus");

        assert_eq!(pdus.len(), 1);
        // Decode to verify it's a SubmitMulti
        use smpp_codec::common::CMD_SUBMIT_MULTI_SM;
        let cmd_id = u32::from_be_bytes([pdus[0][4], pdus[0][5], pdus[0][6], pdus[0][7]]);
        assert_eq!(cmd_id, CMD_SUBMIT_MULTI_SM);
    }
}
