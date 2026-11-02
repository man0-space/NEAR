use crate::error::Man0Error;

pub const RESERVED_KEY_AGENT_WALLET: &str = "agentWallet";

pub const MAX_URI_LEN: usize = 512;
pub const MAX_METADATA_KEY_LEN: usize = 64;
pub const MAX_METADATA_VALUE_LEN: usize = 1024;
pub const MAX_METADATA_ENTRIES: usize = 32;

pub fn validate_uri(uri: &str, allow_empty: bool) -> Result<(), Man0Error> {
    if !allow_empty && uri.trim().is_empty() {
        return Err(Man0Error::EmptyUri);
    }
    if uri.len() > MAX_URI_LEN {
        return Err(Man0Error::UriTooLong);
    }
    Ok(())
}

pub fn is_reserved_metadata_key(key: &str) -> bool {
    key == RESERVED_KEY_AGENT_WALLET
}

pub fn validate_user_metadata_entry(key: &str, value: &[u8]) -> Result<(), Man0Error> {
    if key.is_empty() || key.len() > MAX_METADATA_KEY_LEN {
        return Err(Man0Error::MetadataKeyTooLong);
    }
    if is_reserved_metadata_key(key) {
        return Err(Man0Error::ReservedMetadataKey);
    }
    if value.len() > MAX_METADATA_VALUE_LEN {
        return Err(Man0Error::MetadataValueTooLong);
    }
    Ok(())
}
