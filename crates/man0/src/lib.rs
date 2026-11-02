pub mod error;
pub mod events;
pub mod ext;
pub mod fixed;
pub mod hash;
pub mod metadata;
pub mod state;
pub mod storage;

#[cfg(test)]
mod tests {
    use super::error::Man0Error;
    use super::fixed::{aggregate_summary, from_wad, to_wad, validate_feedback_value, MAX_FEEDBACK_ABS, WAD};
    use super::hash::{
        bytes32_to_hex, derive_clone_dna, derive_registration_dna, derive_wallet_signing_hash,
        derive_warp_dna, hex_to_bytes32, is_zero_hash, ZERO_HASH,
    };
    use super::metadata::{
        is_reserved_metadata_key, validate_uri, validate_user_metadata_entry, MAX_URI_LEN,
        RESERVED_KEY_AGENT_WALLET,
    };
    use near_sdk::AccountId;

    #[test]
    fn test_fixed18_conversions_and_bounds() {
        assert_eq!(to_wad(1, 0).unwrap(), WAD);
        assert_eq!(to_wad(1_500_000, 6).unwrap(), 1_500_000_000_000_000_000);
        assert_eq!(to_wad(-250, 2).unwrap(), -2_500_000_000_000_000_000);
        assert_eq!(from_wad(2_500_000_000_000_000_000, 2).unwrap(), 250);

        assert_eq!(validate_feedback_value(0, 19), Err(Man0Error::InvalidDecimals));
        assert_eq!(
            validate_feedback_value(MAX_FEEDBACK_ABS + 1, 18),
            Err(Man0Error::ValueOutOfBounds)
        );
        assert_eq!(
            validate_feedback_value(-MAX_FEEDBACK_ABS - 1, 18),
            Err(Man0Error::ValueOutOfBounds)
        );
        assert_eq!(
            validate_feedback_value(i128::MIN, 0),
            Err(Man0Error::ValueOutOfBounds)
        );
    }

    #[test]
    fn test_aggregate_summary_mode_decimals_and_signed_average() {
        assert_eq!(aggregate_summary(&[]).unwrap(), (0, 0, 0));

        // 3 entries:
        // +10.00 (1000 with 2 decimals)
        // +20.00 (2000 with 2 decimals)
        // +30.000000 (30_000_000 with 6 decimals)
        // Average = 20.00, mode decimals = 2 -> summary_value = 2000
        let entries = [(1000i128, 2u8), (2000i128, 2u8), (30_000_000i128, 6u8)];
        let (count, val, mode) = aggregate_summary(&entries).unwrap();
        assert_eq!(count, 3);
        assert_eq!(mode, 2);
        assert_eq!(val, 2000);

        // Signed entries: -50 (0 dec) and +150 (0 dec) -> avg = +50 (0 dec)
        let signed_entries = [(-50i128, 0u8), (150i128, 0u8)];
        assert_eq!(aggregate_summary(&signed_entries).unwrap(), (2, 50, 0));
    }

    #[test]
    fn test_hex_and_hash_derivations() {
        assert!(is_zero_hash(&ZERO_HASH));
        let sample = [0xabu8; 32];
        assert!(!is_zero_hash(&sample));

        let hex = bytes32_to_hex(&sample);
        assert_eq!(hex.len(), 66);
        assert!(hex.starts_with("0x"));
        let decoded = hex_to_bytes32(&hex).unwrap();
        assert_eq!(decoded, sample);

        let decoded_no_prefix = hex_to_bytes32(&hex[2..]).unwrap();
        assert_eq!(decoded_no_prefix, sample);

        assert_eq!(hex_to_bytes32("0x1234"), Err(Man0Error::ZeroDnaHash));
        assert_eq!(
            hex_to_bytes32(&"z".repeat(64)),
            Err(Man0Error::ZeroDnaHash)
        );

        let alice: AccountId = "alice.testnet".parse().unwrap();
        let contract: AccountId = "factory.testnet".parse().unwrap();
        let dna1 = derive_registration_dna(&alice, "ipfs://agent1", 1);
        let dna2 = derive_registration_dna(&alice, "ipfs://agent1", 2);
        assert!(!is_zero_hash(&dna1));
        assert_ne!(dna1, dna2);

        let clone_dna = derive_clone_dna(&dna1, 1, "gpt-5", &alice, 1000);
        assert_ne!(clone_dna, dna1);

        let warp_dna = derive_warp_dna(&sample, &alice, 2000);
        assert_ne!(warp_dna, sample);

        let sig_hash = derive_wallet_signing_hash(&contract, 1, &alice, "wallet.testnet", 9999);
        assert!(!is_zero_hash(&sig_hash));
    }

    #[test]
    fn test_metadata_validation_and_reserved_key() {
        assert!(is_reserved_metadata_key(RESERVED_KEY_AGENT_WALLET));
        assert!(!is_reserved_metadata_key("framework"));

        assert_eq!(
            validate_user_metadata_entry(RESERVED_KEY_AGENT_WALLET, b"val"),
            Err(Man0Error::ReservedMetadataKey)
        );
        assert_eq!(
            validate_user_metadata_entry("", b"val"),
            Err(Man0Error::MetadataKeyTooLong)
        );
        assert_eq!(
            validate_user_metadata_entry(&"k".repeat(65), b"val"),
            Err(Man0Error::MetadataKeyTooLong)
        );
        assert_eq!(
            validate_user_metadata_entry("valid_key", &vec![1u8; 1025]),
            Err(Man0Error::MetadataValueTooLong)
        );
        assert!(validate_user_metadata_entry("model", b"claude").is_ok());

        assert!(validate_uri("", true).is_ok());
        assert_eq!(validate_uri("", false), Err(Man0Error::EmptyUri));
        assert_eq!(
            validate_uri(&"a".repeat(MAX_URI_LEN + 1), true),
            Err(Man0Error::UriTooLong)
        );
    }
}
