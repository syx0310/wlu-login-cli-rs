use base64::Engine as _;
use base64::alphabet::Alphabet;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};
use hmac::{Hmac, Mac};
use md5::Md5;
use serde_json::{Map, Value};
use sha1::{Digest, Sha1};

const SRUN_BASE64_ALPHABET: &str =
    "LVoJPiCN2R8G90yg+hmFHuacZ1OWMnrsSTXkYpUq/3dlbfKwv6xztjI7DeBE45QA";

type HmacMd5 = Hmac<Md5>;

pub fn sha1_hex(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    hex_lower(&hasher.finalize())
}

pub fn hmac_md5_hex(data: &str, key: &str) -> String {
    let mut mac = HmacMd5::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

pub fn encrypted_user_info(
    username: &str,
    password: &str,
    ip: &str,
    acid: &str,
    token: &str,
) -> String {
    let mut info = Map::new();
    info.insert("acid".to_owned(), Value::String(acid.to_owned()));
    info.insert("enc_ver".to_owned(), Value::String("srun_bx1".to_owned()));
    info.insert("ip".to_owned(), Value::String(ip.to_owned()));
    info.insert("password".to_owned(), Value::String(password.to_owned()));
    info.insert("username".to_owned(), Value::String(username.to_owned()));
    let json = Value::Object(info).to_string();
    format!("{{SRBX1}}{}", srun_base64_encode(&xencode(&json, token)))
}

pub fn checksum(
    username: &str,
    password_md5: &str,
    acid: &str,
    ip: &str,
    info: &str,
    token: &str,
) -> String {
    let mut content = String::new();
    content.push_str(token);
    content.push_str(username);
    content.push_str(token);
    content.push_str(password_md5);
    content.push_str(token);
    content.push_str(acid);
    content.push_str(token);
    content.push_str(ip);
    content.push_str(token);
    content.push_str("200");
    content.push_str(token);
    content.push('1');
    content.push_str(token);
    content.push_str(info);
    sha1_hex(&content)
}

fn srun_base64_encode(bytes: &[u8]) -> String {
    let alphabet = Alphabet::new(SRUN_BASE64_ALPHABET).expect("valid SRUN base64 alphabet");
    let engine = GeneralPurpose::new(&alphabet, GeneralPurposeConfig::new());
    engine.encode(bytes)
}

pub fn xencode(content: &str, key: &str) -> Vec<u8> {
    if content.is_empty() {
        return Vec::new();
    }

    let mut pwd = sens_code(content, true);
    let mut pwdk = sens_code(key, false);
    while pwdk.len() < 4 {
        pwdk.push(0);
    }

    let n = pwd.len() - 1;
    let mut z = pwd[n];
    let mut y;
    let c = 0x86014019_u32 | 0x183639A0_u32;
    let mut q = (6.0 + 52.0 / (n as f64 + 1.0)).floor() as u32;
    let mut d = 0_u32;

    while q > 0 {
        d = d.wrapping_add(c);
        let e = (d >> 2) & 3;
        let mut p = 0_usize;
        while p < n {
            y = pwd[p + 1];
            let mut m = (z >> 5) ^ (y << 2);
            m = m.wrapping_add(((y >> 3) ^ (z << 4)) ^ (d ^ y));
            m = m.wrapping_add(pwdk[((p as u32 & 3) ^ e) as usize] ^ z);
            pwd[p] = pwd[p].wrapping_add(m);
            z = pwd[p];
            p += 1;
        }

        y = pwd[0];
        let mut m = (z >> 5) ^ (y << 2);
        m = m.wrapping_add(((y >> 3) ^ (z << 4)) ^ (d ^ y));
        m = m.wrapping_add(pwdk[((p as u32 & 3) ^ e) as usize] ^ z);
        pwd[n] = pwd[n].wrapping_add(m);
        z = pwd[n];
        q -= 1;
    }

    len_code(&pwd, false)
}

fn ordat(bytes: &[u8], idx: usize) -> u32 {
    bytes.get(idx).copied().unwrap_or(0) as u32
}

fn sens_code(content: &str, key: bool) -> Vec<u32> {
    let bytes = content.as_bytes();
    let mut pwd = Vec::new();
    for i in (0..bytes.len()).step_by(4) {
        pwd.push(
            ordat(bytes, i)
                | (ordat(bytes, i + 1) << 8)
                | (ordat(bytes, i + 2) << 16)
                | (ordat(bytes, i + 3) << 24),
        );
    }
    if key {
        pwd.push(bytes.len() as u32);
    }
    pwd
}

fn len_code(msg: &[u32], key: bool) -> Vec<u8> {
    let l = msg.len() as u32;
    let mut ll = (l - 1) << 2;
    if key {
        let m = msg[l as usize - 1];
        if m < ll - 3 || m > ll {
            return Vec::new();
        }
        ll = m;
    }

    let mut out = Vec::with_capacity(msg.len() * 4);
    for value in msg {
        out.push((value & 0xff) as u8);
        out.push(((value >> 8) & 0xff) as u8);
        out.push(((value >> 16) & 0xff) as u8);
        out.push(((value >> 24) & 0xff) as u8);
    }

    if key {
        out.truncate(ll as usize);
    }
    out
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_go_implementation() {
        assert_eq!(sha1_hex("test"), "a94a8fe5ccb19ba61c4c0873d391e987982fbbd3");
        assert_eq!(
            hmac_md5_hex("password", "1234567890abcdef"),
            "0277d7f87258748b8d8dd26aeeea398e"
        );
    }

    #[test]
    fn xencode_matches_go_implementation() {
        let encoded = xencode(
            r#"{"acid":"1","enc_ver":"srun_bx1","ip":"10.2.3.4","password":"pass","username":"user"}"#,
            "1234567890abcdef",
        );
        assert_eq!(
            srun_base64_encode(&encoded),
            "dF06LIIjHNlHa6L6Cii6TJUXBMfs1JcWIxVB20E90NoPE8C938SE2Gaz+UkiUHVwmueRDH7+ZzRCUzMD/UJQ5udON2dQ0HY1N8MYaYkVQFndjCVWw4sid2r39kt="
        );
    }
}
