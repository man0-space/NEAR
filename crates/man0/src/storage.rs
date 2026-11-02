use crate::error::Man0Error;
use crate::man0_require;
use near_sdk::{env, AccountId, NearToken, Promise};

/// Verify that at least 1 yoctoNEAR was attached (standard NEP-171 / NEP-141 security check).
pub fn assert_at_least_one_yocto() {
    man0_require!(
        env::attached_deposit() >= NearToken::from_yoctonear(1),
        Man0Error::InsufficientDeposit
    );
}

/// Settle storage delta after flushing `near_sdk::store` collections.
/// If `storage_after > storage_before`, verifies that `attached_deposit >= required_cost + extra_reserved`
/// and refunds any unused excess to `refund_to`.
pub fn settle_storage_and_refund(
    storage_before: u64,
    extra_reserved_yocto: u128,
    refund_to: AccountId,
) {
    let storage_after = env::storage_usage();
    let added_bytes = storage_after.saturating_sub(storage_before);
    let storage_cost_yocto = env::storage_byte_cost()
        .as_yoctonear()
        .saturating_mul(added_bytes as u128);
    let total_required = storage_cost_yocto.saturating_add(extra_reserved_yocto);
    let attached = env::attached_deposit().as_yoctonear();

    man0_require!(attached >= total_required, Man0Error::InsufficientDeposit);

    let refund = attached.saturating_sub(total_required);
    if refund > 1 {
        let _ = Promise::new(refund_to).transfer(NearToken::from_yoctonear(refund));
    }
}
