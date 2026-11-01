use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub const DOMAIN: &[u8] = b"gateveriq:pow:v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub challenge_id: String,
    pub algorithm: String,
    pub salt: String,
    pub difficulty_bits: u8,
    pub expires_at: i64,
}
pub fn random() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    B64.encode(b)
}
pub fn decode(s: &str, len: usize) -> Option<Vec<u8>> {
    let b = B64.decode(s).ok()?;
    (b.len() == len && B64.encode(&b) == s).then_some(b)
}
pub fn hash(b: &[u8]) -> String {
    B64.encode(Sha256::digest(b))
}
pub fn verify(c: &Challenge, nonce: &str) -> bool {
    if c.algorithm != "sha256-pow-v1" || c.difficulty_bits > 24 {
        return false;
    }
    let Some(id) = decode(&c.challenge_id, 32) else {
        return false;
    };
    let Some(salt) = decode(&c.salt, 32) else {
        return false;
    };
    let Some(nonce) = decode(nonce, 8) else {
        return false;
    };
    let mut h = Sha256::new();
    h.update(DOMAIN);
    h.update(id);
    h.update(salt);
    h.update(nonce);
    let digest = h.finalize();
    (0..c.difficulty_bits as usize).all(|i| digest[i / 8] & (0x80 >> (i % 8)) == 0)
}
/// Little-endian counter encoding is a solver convention; all eight-byte nonces are valid.
pub fn solve(c: &Challenge) -> Result<String, &'static str> {
    if c.difficulty_bits > 24
        || decode(&c.challenge_id, 32).is_none()
        || decode(&c.salt, 32).is_none()
        || c.algorithm != "sha256-pow-v1"
    {
        return Err("invalid challenge");
    }
    for n in 0u64.. {
        if n % 4096 == 0 && crate::service::now() >= c.expires_at {
            return Err("challenge expired");
        }
        let nonce = B64.encode(n.to_le_bytes());
        if verify(c, &nonce) {
            return Ok(nonce);
        }
    }
    Err("nonce space exhausted")
}
