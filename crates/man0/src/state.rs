use near_sdk::json_types::{Base64VecU8, I128, U128, U64};
use near_sdk::{near, AccountId};
use std::collections::HashMap;

pub const TREASURY_FEE_BPS: u128 = 1000; // 10%
pub const CREATOR_SHARE_BPS: u128 = 9000; // 90%
pub const BPS_DENOMINATOR: u128 = 10_000;

pub const WARP_WARPER_SHARE_BPS: u128 = 8000; // 80%
pub const WARP_CREATOR_SHARE_BPS: u128 = 1000; // 10%
pub const WARP_TREASURY_SHARE_BPS: u128 = 1000; // 10%
pub const WARP_CLAIM_WINDOW_NS: u64 = 365 * 86_400 * 1_000_000_000; // 365 days in nanoseconds

pub const NANOS_PER_DAY: u64 = 86_400 * 1_000_000_000;
pub const MAX_LEASE_PERCENT: u8 = 20;
pub const MAX_NESTING_DEPTH: u8 = 32;
pub const DEFAULT_MAX_PENDING_CHILDREN: u16 = 128;

pub const USDC_DECIMALS: u8 = 6;

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataEntryInput {
    pub key: String,
    pub value: Base64VecU8,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AgentMetadataKey {
    pub agent_id: u64,
    pub key: String,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LicenseKey {
    pub agent_id: u64,
    pub workflow_contract: AccountId,
    pub workflow_id: u64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseRecord {
    pub agent_id: u64,
    pub workflow_contract: AccountId,
    pub workflow_id: u64,
    pub licensed_at: U64,
    pub is_active: bool,
    pub price_paid: U128,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRecord {
    pub agent_id: u64,
    pub dna_hash: [u8; 32],
    pub owner: AccountId,
    pub creator: AccountId,
    pub licenses: u64,
    pub licenses_minted: u64,
    pub license_price: u128,
    pub creator_fee: u64,
    pub cloneable: bool,
    pub is_clone: bool,
    pub parent_agent_id: u64,
    pub agent_wallet: Option<String>,
    pub uri: String,
    pub created_at: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentView {
    pub agent_id: u64,
    pub dna_hash: String,
    pub owner: AccountId,
    pub creator: AccountId,
    pub licenses: u64,
    pub licenses_minted: u64,
    pub license_price: U128,
    pub creator_fee: u64,
    pub cloneable: bool,
    pub is_clone: bool,
    pub parent_agent_id: u64,
    pub agent_wallet: String,
    pub uri: String,
    pub created_at: U64,
}

impl AgentRecord {
    pub fn to_view(&self) -> AgentView {
        AgentView {
            agent_id: self.agent_id,
            dna_hash: crate::hash::bytes32_to_hex(&self.dna_hash),
            owner: self.owner.clone(),
            creator: self.creator.clone(),
            licenses: self.licenses,
            licenses_minted: self.licenses_minted,
            license_price: U128(self.license_price),
            creator_fee: self.creator_fee,
            cloneable: self.cloneable,
            is_clone: self.is_clone,
            parent_agent_id: self.parent_agent_id,
            agent_wallet: self.agent_wallet.clone().unwrap_or_default(),
            uri: self.uri.clone(),
            created_at: U64(self.created_at),
        }
    }
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentPermissionInfo {
    pub exists: bool,
    pub owner: AccountId,
    pub creator: AccountId,
    pub agent_wallet: String,
    pub is_approved_operator: bool,
    pub license_price: U128,
    pub is_warped: bool,
    pub warp_creator: Option<AccountId>,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarpRecord {
    pub warped_agent_id: u64,
    pub original_hash: [u8; 32],
    pub original_creator: Option<AccountId>,
    pub warper: AccountId,
    pub warped_at: u64,
    pub royalty_expiry: u64,
    pub royalties_claimed: bool,
    pub accumulated_royalties: u128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarpRecordView {
    pub warped_agent_id: u64,
    pub original_hash: String,
    pub original_creator: Option<AccountId>,
    pub warper: AccountId,
    pub warped_at: U64,
    pub royalty_expiry: U64,
    pub royalties_claimed: bool,
    pub accumulated_royalties: U128,
}

impl WarpRecord {
    pub fn to_view(&self) -> WarpRecordView {
        WarpRecordView {
            warped_agent_id: self.warped_agent_id,
            original_hash: crate::hash::bytes32_to_hex(&self.original_hash),
            original_creator: self.original_creator.clone(),
            warper: self.warper.clone(),
            warped_at: U64(self.warped_at),
            royalty_expiry: U64(self.royalty_expiry),
            royalties_claimed: self.royalties_claimed,
            accumulated_royalties: U128(self.accumulated_royalties),
        }
    }
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarpSplitView {
    pub warper: AccountId,
    pub warper_amount: U128,
    pub creator: Option<AccountId>,
    pub creator_amount: U128,
    pub treasury: AccountId,
    pub treasury_amount: U128,
    pub unclaimed_creator_share: U128,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloneParams {
    pub chain_id: u64,
    pub license_price: U128,
    pub model: String,
    pub licenses: u64,
}

// =============================================================================
// Reputation Types (ERC-8004)
// =============================================================================

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FeedbackKey {
    pub agent_id: u64,
    pub client: AccountId,
    pub feedback_index: u64,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackRecord {
    pub agent_id: u64,
    pub client: AccountId,
    pub feedback_index: u64,
    pub value: i128,
    pub value_decimals: u8,
    pub tag1: String,
    pub tag2: String,
    pub endpoint: String,
    pub feedback_uri: String,
    pub feedback_hash: [u8; 32],
    pub is_revoked: bool,
    pub created_at: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedbackView {
    pub agent_id: u64,
    pub client: AccountId,
    pub feedback_index: u64,
    pub value: I128,
    pub value_decimals: u8,
    pub tag1: String,
    pub tag2: String,
    pub endpoint: String,
    pub feedback_uri: String,
    pub feedback_hash: String,
    pub is_revoked: bool,
    pub created_at: U64,
}

impl FeedbackRecord {
    pub fn to_view(&self) -> FeedbackView {
        FeedbackView {
            agent_id: self.agent_id,
            client: self.client.clone(),
            feedback_index: self.feedback_index,
            value: I128(self.value),
            value_decimals: self.value_decimals,
            tag1: self.tag1.clone(),
            tag2: self.tag2.clone(),
            endpoint: self.endpoint.clone(),
            feedback_uri: self.feedback_uri.clone(),
            feedback_hash: crate::hash::bytes32_to_hex(&self.feedback_hash),
            is_revoked: self.is_revoked,
            created_at: U64(self.created_at),
        }
    }
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SummaryResult {
    pub count: u64,
    pub summary_value: I128,
    pub summary_value_decimals: u8,
}

// =============================================================================
// Validation Types (ERC-8004)
// =============================================================================

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationStatus {
    Pending,
    Completed,
    Closed,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationRequest {
    pub request_id: u64,
    pub agent_id: u64,
    pub requester: AccountId,
    pub validator_type: String,
    pub task_hash: [u8; 32],
    pub request_uri: String,
    pub status: ValidationStatus,
    pub created_at: u64,
    pub response_count: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationRequestView {
    pub request_id: u64,
    pub agent_id: u64,
    pub requester: AccountId,
    pub validator_type: String,
    pub task_hash: String,
    pub request_uri: String,
    pub status: ValidationStatus,
    pub created_at: U64,
    pub response_count: u64,
}

impl ValidationRequest {
    pub fn to_view(&self) -> ValidationRequestView {
        ValidationRequestView {
            request_id: self.request_id,
            agent_id: self.agent_id,
            requester: self.requester.clone(),
            validator_type: self.validator_type.clone(),
            task_hash: crate::hash::bytes32_to_hex(&self.task_hash),
            request_uri: self.request_uri.clone(),
            status: self.status,
            created_at: U64(self.created_at),
            response_count: self.response_count,
        }
    }
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationResponse {
    pub response_id: u64,
    pub request_id: u64,
    pub validator: AccountId,
    pub valid: bool,
    pub evidence_hash: [u8; 32],
    pub evidence_uri: String,
    pub responded_at: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationResponseView {
    pub response_id: u64,
    pub request_id: u64,
    pub validator: AccountId,
    pub valid: bool,
    pub evidence_hash: String,
    pub evidence_uri: String,
    pub responded_at: U64,
}

impl ValidationResponse {
    pub fn to_view(&self) -> ValidationResponseView {
        ValidationResponseView {
            response_id: self.response_id,
            request_id: self.request_id,
            validator: self.validator.clone(),
            valid: self.valid,
            evidence_hash: crate::hash::bytes32_to_hex(&self.evidence_hash),
            evidence_uri: self.evidence_uri.clone(),
            responded_at: U64(self.responded_at),
        }
    }
}

// =============================================================================
// Workflow, ERC-7401 Nesting, RFA, and Lease Types
// =============================================================================

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Child {
    pub contract_id: AccountId,
    pub token_id: u64,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectOwner {
    pub owner_id: AccountId,
    pub parent_token_id: u64,
    pub is_nft: bool,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintWorkflowParams {
    pub title: String,
    pub description: String,
    pub banner: String,
    pub workflow_card_uri: String,
    pub units: u64,
    pub lease_enabled: bool,
    pub lease_duration: u64,
    pub lease_percent: u8,
    pub has_coordinator: bool,
    pub coordinator_model: String,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowRecord {
    pub workflow_id: u64,
    pub title: String,
    pub description: String,
    pub banner: String,
    pub workflow_card_uri: String,
    pub total_price: u128,
    pub units: u64,
    pub units_minted: u64,
    pub creator: AccountId,
    pub owner: AccountId,
    pub lease_enabled: bool,
    pub lease_duration: u64,
    pub lease_percent: u8,
    pub has_coordinator: bool,
    pub coordinator_model: String,
    pub has_active_rfa: bool,
    pub rfa_id: u64,
    pub created_at: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowView {
    pub workflow_id: u64,
    pub title: String,
    pub description: String,
    pub banner: String,
    pub workflow_card_uri: String,
    pub total_price: U128,
    pub units: u64,
    pub units_minted: u64,
    pub creator: AccountId,
    pub owner: AccountId,
    pub lease_enabled: bool,
    pub lease_duration: u64,
    pub lease_percent: u8,
    pub has_coordinator: bool,
    pub coordinator_model: String,
    pub has_active_rfa: bool,
    pub rfa_id: u64,
    pub created_at: U64,
}

impl WorkflowRecord {
    pub fn to_view(&self) -> WorkflowView {
        WorkflowView {
            workflow_id: self.workflow_id,
            title: self.title.clone(),
            description: self.description.clone(),
            banner: self.banner.clone(),
            workflow_card_uri: self.workflow_card_uri.clone(),
            total_price: U128(self.total_price),
            units: self.units,
            units_minted: self.units_minted,
            creator: self.creator.clone(),
            owner: self.owner.clone(),
            lease_enabled: self.lease_enabled,
            lease_duration: self.lease_duration,
            lease_percent: self.lease_percent,
            has_coordinator: self.has_coordinator,
            coordinator_model: self.coordinator_model.clone(),
            has_active_rfa: self.has_active_rfa,
            rfa_id: self.rfa_id,
            created_at: U64(self.created_at),
        }
    }
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RfaStatus {
    Open,
    Fulfilled,
    Cancelled,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RfaRecord {
    pub rfa_id: u64,
    pub workflow_id: u64,
    pub title: String,
    pub description: String,
    pub required_skills: Vec<String>,
    pub offer_amount: u128,
    pub publisher: AccountId,
    pub created_at: u64,
    pub status: RfaStatus,
    pub fulfilled_by_agent_id: u64,
    pub agent_creator: Option<AccountId>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RfaView {
    pub rfa_id: u64,
    pub workflow_id: u64,
    pub title: String,
    pub description: String,
    pub required_skills: Vec<String>,
    pub offer_amount: U128,
    pub publisher: AccountId,
    pub created_at: U64,
    pub status: RfaStatus,
    pub fulfilled_by_agent_id: u64,
    pub agent_creator: Option<AccountId>,
}

impl RfaRecord {
    pub fn to_view(&self) -> RfaView {
        RfaView {
            rfa_id: self.rfa_id,
            workflow_id: self.workflow_id,
            title: self.title.clone(),
            description: self.description.clone(),
            required_skills: self.required_skills.clone(),
            offer_amount: U128(self.offer_amount),
            publisher: self.publisher.clone(),
            created_at: U64(self.created_at),
            status: self.status,
            fulfilled_by_agent_id: self.fulfilled_by_agent_id,
            agent_creator: self.agent_creator.clone(),
        }
    }
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submission {
    pub agent_id: u64,
    pub creator: AccountId,
    pub submitted_at: U64,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseRecord {
    pub lease_id: u64,
    pub workflow_id: u64,
    pub leaser: AccountId,
    pub start_time: u64,
    pub end_time: u64,
    pub creator_percent: u8,
    pub is_active: bool,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseView {
    pub lease_id: u64,
    pub workflow_id: u64,
    pub leaser: AccountId,
    pub start_time: U64,
    pub end_time: U64,
    pub creator_percent: u8,
    pub is_active: bool,
}

impl LeaseRecord {
    pub fn to_view(&self) -> LeaseView {
        LeaseView {
            lease_id: self.lease_id,
            workflow_id: self.workflow_id,
            leaser: self.leaser.clone(),
            start_time: U64(self.start_time),
            end_time: U64(self.end_time),
            creator_percent: self.creator_percent,
            is_active: self.is_active,
        }
    }
}

// =============================================================================
// NEP-171 / NEP-177 / NEP-178 / NEP-181 Standard NFT Types
// =============================================================================

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NFTContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<Base64VecU8>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub media: Option<String>,
    pub media_hash: Option<Base64VecU8>,
    pub copies: Option<u64>,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub starts_at: Option<String>,
    pub updated_at: Option<String>,
    pub extra: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<Base64VecU8>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonToken {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: TokenMetadata,
    pub approved_account_ids: HashMap<AccountId, u64>,
}
