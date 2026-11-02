use crate::error::Man0Error;
use near_sdk::{env, AccountId};

pub const ZERO_HASH: [u8; 32] = [0u8; 32];

pub fn is_zero_hash(hash: &[u8; 32]) -> bool {
    *hash == ZERO_HASH
}

pub fn bytes32_to_hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(66);
    out.push_str("0x");
    for b in bytes {
        use core::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

pub fn hex_to_bytes32(input: &str) -> Result<[u8; 32], Man0Error> {
    let stripped = input.strip_prefix("0x").unwrap_or(input);
    if stripped.len() != 64 {
        return Err(Man0Error::ZeroDnaHash);
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let byte_str = &stripped[i * 2..i * 2 + 2];
        out[i] = u8::from_str_radix(byte_str, 16).map_err(|_| Man0Error::ZeroDnaHash)?;
    }
    Ok(out)
}

/// Deterministic DNA hash for `register_agent`:
/// `sha256("ERC8004_MAN0" || owner || uri || agent_id_be_bytes)`
pub fn derive_registration_dna(owner: &AccountId, uri: &str, agent_id: u64) -> [u8; 32] {
    let mut buf = Vec::with_capacity(32 + owner.as_str().len() + uri.len());
    buf.extend_from_slice(b"ERC8004_MAN0:");
    buf.extend_from_slice(owner.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(uri.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(&agent_id.to_be_bytes());
    env::sha256_array(&buf)
}

/// Deterministic DNA hash for `clone_agent`:
/// `sha256("CLONE_MAN0" || parent_dna_hash || chain_id || model || cloner || timestamp_ns)`
pub fn derive_clone_dna(
    parent_dna_hash: &[u8; 32],
    chain_id: u64,
    model: &str,
    cloner: &AccountId,
    timestamp_ns: u64,
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(64 + model.len() + cloner.as_str().len());
    buf.extend_from_slice(b"CLONE_MAN0:");
    buf.extend_from_slice(parent_dna_hash);
    buf.extend_from_slice(&chain_id.to_be_bytes());
    buf.extend_from_slice(model.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(cloner.as_bytes());
    buf.extend_from_slice(&timestamp_ns.to_be_bytes());
    env::sha256_array(&buf)
}

/// Deterministic DNA hash for `warp_agent`:
/// `sha256("WARPED_MAN0" || original_hash || warper || timestamp_ns)`
pub fn derive_warp_dna(
    original_hash: &[u8; 32],
    warper: &AccountId,
    timestamp_ns: u64,
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(64 + warper.as_str().len());
    buf.extend_from_slice(b"WARPED_MAN0:");
    buf.extend_from_slice(original_hash);
    buf.extend_from_slice(warper.as_bytes());
    buf.extend_from_slice(&timestamp_ns.to_be_bytes());
    env::sha256_array(&buf)
}

/// Signing payload for `set_agent_wallet` Ed25519 signature verification:
/// `sha256("ERC8004_SET_WALLET" || contract_id || agent_id || owner || new_wallet || deadline_ns)`
pub fn derive_wallet_signing_hash(
    contract_id: &AccountId,
    agent_id: u64,
    owner: &AccountId,
    new_wallet: &str,
    deadline_ns: u64,
) -> [u8; 32] {
    let mut buf = Vec::with_capacity(
        32 + contract_id.as_str().len() + owner.as_str().len() + new_wallet.len(),
    );
    buf.extend_from_slice(b"ERC8004_SET_WALLET:");
    buf.extend_from_slice(contract_id.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(&agent_id.to_be_bytes());
    buf.push(b':');
    buf.extend_from_slice(owner.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(new_wallet.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(&deadline_ns.to_be_bytes());
    env::sha256_array(&buf)
}
