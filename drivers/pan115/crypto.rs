//! 115云盘加密模块
//! 完全照抄OpenList的m115 XOR+RSA加密实现

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sha1::{Sha1, Digest as Sha1Digest};
use num_bigint::BigUint;
use rand::RngCore;
use anyhow::{Result, anyhow};
use std::io::Read;

const XOR_KEY_SEED: [u8; 144] = [
    0xf0, 0xe5, 0x69, 0xae, 0xbf, 0xdc, 0xbf, 0x8a,
    0x1a, 0x45, 0xe8, 0xbe, 0x7d, 0xa6, 0x73, 0xb8,
    0xde, 0x8f, 0xe7, 0xc4, 0x45, 0xda, 0x86, 0xc4,
    0x9b, 0x64, 0x8b, 0x14, 0x6a, 0xb4, 0xf1, 0xaa,
    0x38, 0x01, 0x35, 0x9e, 0x26, 0x69, 0x2c, 0x86,
    0x00, 0x6b, 0x4f, 0xa5, 0x36, 0x34, 0x62, 0xa6,
    0x2a, 0x96, 0x68, 0x18, 0xf2, 0x4a, 0xfd, 0xbd,
    0x6b, 0x97, 0x8f, 0x4d, 0x8f, 0x89, 0x13, 0xb7,
    0x6c, 0x8e, 0x93, 0xed, 0x0e, 0x0d, 0x48, 0x3e,
    0xd7, 0x2f, 0x88, 0xd8, 0xfe, 0xfe, 0x7e, 0x86,
    0x50, 0x95, 0x4f, 0xd1, 0xeb, 0x83, 0x26, 0x34,
    0xdb, 0x66, 0x7b, 0x9c, 0x7e, 0x9d, 0x7a, 0x81,
    0x32, 0xea, 0xb6, 0x33, 0xde, 0x3a, 0xa9, 0x59,
    0x34, 0x66, 0x3b, 0xaa, 0xba, 0x81, 0x60, 0x48,
    0xb9, 0xd5, 0x81, 0x9c, 0xf8, 0x6c, 0x84, 0x77,
    0xff, 0x54, 0x78, 0x26, 0x5f, 0xbe, 0xe8, 0x1e,
    0x36, 0x9f, 0x34, 0x80, 0x5c, 0x45, 0x2c, 0x9b,
    0x76, 0xd5, 0x1b, 0x8f, 0xcc, 0xc3, 0xb8, 0xf5,
];

const XOR_CLIENT_KEY: [u8; 12] = [
    0x78, 0x06, 0xad, 0x4c, 0x33, 0x86, 0x5d, 0x18,
    0x4c, 0x01, 0x3f, 0x46,
];

const RSA_N_HEX: &str = "8686980c0f5a24c4b9d43020cd2c22703ff3f450756529058b1cf88f09b8602136477198a6e2683149659bd122c33592fdb5ad47944ad1ea4d36c6b172aad6338c3bb6ac6227502d010993ac967d1aef00f0c8e038de2e4d3bc2ec368af2e9f10a6f1eda4f7262f136420c07c331b871bf139f74f3010e3c4fe57df3afb71683";
const RSA_E: u32 = 0x10001;

fn xor_derive_key(seed: &[u8], size: usize) -> Vec<u8> {
    let mut key = vec![0u8; size];
    for i in 0..size {
        key[i] = (seed[i].wrapping_add(XOR_KEY_SEED[size * i])) & 0xff;
        key[i] ^= XOR_KEY_SEED[size * (size - i - 1)];
    }
    key
}

fn xor_transform(data: &mut [u8], key: &[u8]) {
    let data_size = data.len();
    let key_size = key.len();
    let mod_val = data_size % 4;
    
    if mod_val > 0 {
        for i in 0..mod_val {
            data[i] ^= key[i % key_size];
        }
    }
    for i in mod_val..data_size {
        data[i] ^= key[(i - mod_val) % key_size];
    }
}

fn reverse_bytes(data: &mut [u8]) {
    data.reverse();
}

fn rsa_encrypt(input: &[u8]) -> Vec<u8> {
    let n = BigUint::parse_bytes(RSA_N_HEX.as_bytes(), 16).unwrap();
    let e = BigUint::from(RSA_E);
    let key_length = ((n.bits() + 7) / 8) as usize;
    
    let mut result = Vec::new();
    let mut remaining = input.len();
    let mut offset = 0;
    
    while remaining > 0 {
        let slice_size = (key_length - 11).min(remaining);
        let encrypted = rsa_encrypt_slice(&input[offset..offset + slice_size], &n, &e, key_length);
        result.extend_from_slice(&encrypted);
        offset += slice_size;
        remaining -= slice_size;
    }
    
    result
}

fn rsa_encrypt_slice(input: &[u8], n: &BigUint, e: &BigUint, key_length: usize) -> Vec<u8> {
    let pad_size = key_length - input.len() - 3;
    let mut pad_data = vec![0u8; pad_size];
    rand::thread_rng().fill_bytes(&mut pad_data);
    
    let mut buf = vec![0u8; key_length];
    buf[0] = 0;
    buf[1] = 2;
    for i in 0..pad_size {
        buf[2 + i] = (pad_data[i] % 0xff) + 0x01;
    }
    buf[pad_size + 2] = 0;
    buf[pad_size + 3..].copy_from_slice(input);
    
    let msg = BigUint::from_bytes_be(&buf);
    let ret = msg.modpow(e, n);
    let ret_bytes = ret.to_bytes_be();
    
    let mut result = vec![0u8; key_length];
    let fill_size = key_length - ret_bytes.len();
    result[fill_size..].copy_from_slice(&ret_bytes);
    
    result
}

fn rsa_decrypt(input: &[u8]) -> Vec<u8> {
    let n = BigUint::parse_bytes(RSA_N_HEX.as_bytes(), 16).unwrap();
    let e = BigUint::from(RSA_E);
    let key_length = ((n.bits() + 7) / 8) as usize;
    
    let mut result = Vec::new();
    let mut remaining = input.len();
    let mut offset = 0;
    
    while remaining > 0 {
        let slice_size = key_length.min(remaining);
        let decrypted = rsa_decrypt_slice(&input[offset..offset + slice_size], &n, &e);
        result.extend_from_slice(&decrypted);
        offset += slice_size;
        remaining -= slice_size;
    }
    
    result
}

fn rsa_decrypt_slice(input: &[u8], n: &BigUint, e: &BigUint) -> Vec<u8> {
    let msg = BigUint::from_bytes_be(input);
    let ret = msg.modpow(e, n);
    let ret_bytes = ret.to_bytes_be();
    
    for (i, &b) in ret_bytes.iter().enumerate() {
        if b == 0 && i != 0 {
            return ret_bytes[i + 1..].to_vec();
        }
    }
    Vec::new()
}

pub fn generate_random_key() -> Vec<u8> {
    let mut key = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

pub fn m115_encode(input: &[u8], key: &[u8]) -> Result<String> {
    let mut buf = Vec::with_capacity(16 + input.len());
    buf.extend_from_slice(key);
    buf.extend_from_slice(input);
    
    let data_part = &mut buf[16..];
    xor_transform(data_part, &xor_derive_key(key, 4));
    reverse_bytes(data_part);
    xor_transform(data_part, &XOR_CLIENT_KEY);
    
    let encrypted = rsa_encrypt(&buf);
    Ok(BASE64.encode(&encrypted))
}

pub fn m115_decode(input: &str, key: &[u8]) -> Result<Vec<u8>> {
    let data = BASE64.decode(input)?;
    let decrypted = rsa_decrypt(&data);
    
    if decrypted.len() <= 16 {
        return Err(anyhow!("Invalid decrypted data"));
    }
    
    let stored_key = &decrypted[..16];
    let mut output = decrypted[16..].to_vec();
    
    xor_transform(&mut output, &xor_derive_key(stored_key, 12));
    reverse_bytes(&mut output);
    xor_transform(&mut output, &xor_derive_key(key, 4));
    
    Ok(output)
}

pub fn sha1_hash(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    format!("{:X}", hasher.finalize())
}

pub fn sha1_hash_lower(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn md5_hash(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

pub fn md5_hash_upper(data: &[u8]) -> String {
    format!("{:X}", md5::compute(data))
}

pub fn sha1_reader<R: Read>(reader: &mut R) -> Result<String> {
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:X}", hasher.finalize()))
}

const MD5_SALT: &str = "Qclm8MGWUv59TnrR0XPg";

pub fn generate_token(
    user_id: i64,
    file_id: &str,
    _pre_id: &str,
    timestamp: &str,
    file_size: &str,
    sign_key: &str,
    sign_val: &str,
    app_ver: &str,
) -> String {
    let user_id_str = user_id.to_string();
    let user_id_md5 = md5_hash(user_id_str.as_bytes());
    
    let token_data = format!(
        "{}{}{}{}{}{}{}{}{}",
        MD5_SALT,
        file_id,
        file_size,
        sign_key,
        sign_val,
        user_id_str,
        timestamp,
        user_id_md5,
        app_ver
    );
    
    md5_hash(token_data.as_bytes())
}

pub fn generate_signature(user_id: i64, file_id: &str, target: &str) -> String {
    let app_ver = "30.2.0";
    let sig_data = format!("{}{}{}{}", user_id, file_id, target, app_ver);
    sha1_hash(sig_data.as_bytes())
}

pub fn calc_file_sha1<R: Read>(reader: &mut R, size: i64) -> Result<String> {
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 65536];
    let mut remaining = size as usize;
    
    while remaining > 0 {
        let to_read = remaining.min(buffer.len());
        let n = reader.read(&mut buffer[..to_read])?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        remaining -= n;
    }
    
    Ok(format!("{:X}", hasher.finalize()))
}

pub fn calc_pre_hash<R: Read>(reader: &mut R, size: i64) -> Result<String> {
    const PRE_HASH_SIZE: i64 = 128 * 1024;
    let hash_size = size.min(PRE_HASH_SIZE) as usize;
    let mut buffer = vec![0u8; hash_size];
    reader.read_exact(&mut buffer)?;
    Ok(sha1_hash(&buffer))
}

pub fn calc_range_sha1<R: Read + std::io::Seek>(reader: &mut R, start: i64, length: i64) -> Result<String> {
    use std::io::SeekFrom;
    reader.seek(SeekFrom::Start(start as u64))?;
    
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 65536];
    let mut remaining = length as usize;
    
    while remaining > 0 {
        let to_read = remaining.min(buffer.len());
        let n = reader.read(&mut buffer[..to_read])?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        remaining -= n;
    }
    
    Ok(format!("{:X}", hasher.finalize()))
}
