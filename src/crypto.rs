/// XChaCha20-Poly1305 encryption / decryption compatible with the PlainApp server.
///
/// Wire format (matches `plain-web` crypto.ts and Android `CryptoHelper.kt`):
///   `nonce (24 bytes) || ciphertext+tag (16 bytes)`
///
/// Payload envelope sent to `ReplayGuard.kt`:
///   `"<unix_ms>|<16-hex-nonce>|<json_body>"`
use crate::Result;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    Key, XChaCha20Poly1305, XNonce,
};
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

// -- public API ---------------------------------------------------------------

/// Decode a standard base64 string (RFC 4648) into bytes.
/// Inline implementation -- no external crate required.
pub fn base64_decode(input: &str) -> Result<Vec<u8>> {
    let mut lut = [0xffu8; 256];
    for (i, &c) in
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
            .iter()
            .enumerate()
    {
        lut[c as usize] = i as u8;
    }
    lut[b'=' as usize] = 0;

    let clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();

    if clean.len() % 4 != 0 {
        return Err(format!("invalid base64 length ({})", clean.len()).into());
    }
    for &b in &clean {
        if b != b'=' && (b > 127 || lut[b as usize] == 0xff) {
            return Err(format!("invalid base64 character '{}'", b as char).into());
        }
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks(4) {
        let [a, b, c, d] = [
            lut[chunk[0] as usize],
            lut[chunk[1] as usize],
            lut[chunk[2] as usize],
            lut[chunk[3] as usize],
        ];
        out.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            out.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            out.push((c << 6) | d);
        }
    }
    Ok(out)
}

/// Wrap a JSON body in the anti-replay envelope required by `ReplayGuard.kt`:
///   `"<unix_ms>|<16-hex-nonce>|<json>"`
pub fn wrap_payload(json: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let nonce = hex_nonce();
    format!("{ts}|{nonce}|{json}")
}

/// Encrypt `plaintext` with XChaCha20-Poly1305.
/// Returns `random_nonce (24 bytes) || ciphertext+tag`.
pub fn encrypt(key: &[u8], plaintext: &str) -> Result<Vec<u8>> {
    let nonce_bytes = random24()?;
    let cipher = build_cipher(key)?;
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("encrypt failed: {e}"))?;
    let mut out = Vec::with_capacity(24 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt bytes produced by [`encrypt`].
/// Input must be `nonce (24 bytes) || ciphertext+tag`.
pub fn decrypt(key: &[u8], data: &[u8]) -> Result<String> {
    if data.len() < 25 {
        return Err("response too short to decrypt".into());
    }
    let cipher = build_cipher(key)?;
    let nonce = XNonce::from_slice(&data[..24]);
    let plain = cipher
        .decrypt(nonce, &data[24..])
        .map_err(|_| "decrypt failed -- wrong token / client_id or corrupted data")?;
    String::from_utf8(plain).map_err(|e| format!("response is not UTF-8: {e}").into())
}

// -- helpers ------------------------------------------------------------------

fn build_cipher(key: &[u8]) -> Result<XChaCha20Poly1305> {
    if key.len() < 32 {
        return Err(format!("token too short: {} bytes (need >=32)", key.len()).into());
    }
    Ok(XChaCha20Poly1305::new(Key::from_slice(&key[..32])))
}

/// Read 24 random bytes from `/dev/urandom` (available on macOS and Linux).
fn random24() -> Result<[u8; 24]> {
    let mut buf = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|e| format!("cannot read /dev/urandom: {e}"))?;
    Ok(buf)
}

/// Generate a 16-char hex nonce (low nibble of each random byte), matching
/// `generateNonce()` in `time-sync.ts`.
fn hex_nonce() -> String {
    let mut buf = [0u8; 16];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf));
    buf.iter().map(|b| format!("{:x}", b & 0x0f)).collect()
}
