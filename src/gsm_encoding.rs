use std::collections::HashMap;

const GSM_BASIC_CHARSET: &str = "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞ\x1bÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ`¿abcdefghijklmnopqrstuvwxyzäöñüà";

pub fn gsm_7bit_encode(text: &str) -> Result<Vec<u8>, String> {
    let mut encoded_text = Vec::new();
    
    // Extended charset mapping
    let mut gsm_extended_charset = HashMap::new();
    gsm_extended_charset.insert('^', 20);
    gsm_extended_charset.insert('{', 40);
    gsm_extended_charset.insert('}', 41);
    gsm_extended_charset.insert('\\', 47);
    gsm_extended_charset.insert('[', 60);
    gsm_extended_charset.insert('~', 61);
    gsm_extended_charset.insert(']', 62);
    gsm_extended_charset.insert('|', 64);
    gsm_extended_charset.insert('€', 101);

    for char in text.chars() {
        if let Some(index) = GSM_BASIC_CHARSET.find(char) {
            encoded_text.push(index as u8);
        } else if let Some(&code) = gsm_extended_charset.get(&char) {
            encoded_text.push(0x1B); // Escape character
            encoded_text.push(code);
        } else {
             // Fallback for demo purposes, or return error as requested
            return Err(format!("Character '{}' not supported in GSM 03.38", char));
        }
    }
    
    Ok(encoded_text)
}

pub fn encode_8bit(text: &str) -> Vec<u8> {
    // Latin-1 (ISO-8859-1) approximation - taking valid bytes or '?'
    text.chars().map(|c| {
        let u = c as u32;
        if u <= 0xFF {
            u as u8
        } else {
            b'?'
        }
    }).collect()
}

pub fn encode_16bit(text: &str) -> Vec<u8> {
    // UCS-2 (Basic Multilingual Plane of UTF-16) - Big Endian
    text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}
