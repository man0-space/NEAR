use man0::error::Man0Error;
use man0::events::{Man0Event, Nep171Event};
use man0::ext::{ext_ft, ext_nft_approval_receiver, ext_nft_receiver, ext_nft_resolver};
use man0::hash::{
    bytes32_to_hex, derive_clone_dna, derive_registration_dna, derive_wallet_signing_hash,
    derive_warp_dna, hex_to_bytes32, is_zero_hash,
};
use man0::man0_require;
use man0::metadata::{
    validate_uri, validate_user_metadata_entry, MAX_METADATA_ENTRIES, RESERVED_KEY_AGENT_WALLET,
};
use man0::state::{
    AgentMetadataKey, AgentPermissionInfo, AgentRecord, AgentView, CloneParams, JsonToken,
    LicenseKey, LicenseRecord, MetadataEntryInput, NFTContractMetadata, TokenMetadata, WarpRecord,
    WarpRecordView, WarpSplitView, BPS_DENOMINATOR, WARP_CLAIM_WINDOW_NS, WARP_CREATOR_SHARE_BPS,
    WARP_WARPER_SHARE_BPS,
};
use man0::storage::assert_at_least_one_yocto;
use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::store::{IterableSet, LookupMap};
use near_sdk::{
    env, near, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};
use std::collections::HashMap;

const GAS_FOR_NFT_ON_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(15);
const GAS_FOR_NFT_ON_APPROVE: Gas = Gas::from_tgas(15);
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(15);

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Agents,
    DnaToAgent,
    AgentMetadata,
    AgentMetadataKeys,
    Licenses,
    ClonesOf,
    WarpRecords,
    OriginalHashToAgent,
    TokensPerOwner,
    OwnerSet { account_hash: [u8; 32] },
    TokenApprovals,
    AuthorizedConsumers,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct FactoryContract {
    admin: AccountId,
    treasury: AccountId,
    payment_token: Option<AccountId>,
    paused: bool,
    next_agent_id: u64,
    total_agents: u64,
    total_clones: u64,
    total_warped: u64,
    next_approval_id: u64,
    agents: LookupMap<u64, AgentRecord>,
    dna_to_agent: LookupMap<[u8; 32], u64>,
    agent_metadata: LookupMap<AgentMetadataKey, Vec<u8>>,
    agent_metadata_keys: LookupMap<u64, Vec<String>>,
    licenses: LookupMap<LicenseKey, LicenseRecord>,
    clones_of: LookupMap<u64, Vec<u64>>,
    warp_records: LookupMap<u64, WarpRecord>,
    original_hash_to_agent: LookupMap<[u8; 32], u64>,
    tokens_per_owner: LookupMap<AccountId, IterableSet<u64>>,
    token_approvals: LookupMap<u64, HashMap<AccountId, u64>>,
    authorized_consumers: IterableSet<AccountId>,
}

#[near]
impl FactoryContract {
    #[init]
    pub fn new(
        admin: AccountId,
        treasury: AccountId,
        payment_token: Option<AccountId>,
    ) -> Self {
        Self {
            admin,
            treasury,
            payment_token,
            paused: false,
            next_agent_id: 1,
            total_agents: 0,
            total_clones: 0,
            total_warped: 0,
            next_approval_id: 1,
            agents: LookupMap::new(StorageKey::Agents),
            dna_to_agent: LookupMap::new(StorageKey::DnaToAgent),
            agent_metadata: LookupMap::new(StorageKey::AgentMetadata),
            agent_metadata_keys: LookupMap::new(StorageKey::AgentMetadataKeys),
            licenses: LookupMap::new(StorageKey::Licenses),
            clones_of: LookupMap::new(StorageKey::ClonesOf),
            warp_records: LookupMap::new(StorageKey::WarpRecords),
            original_hash_to_agent: LookupMap::new(StorageKey::OriginalHashToAgent),
            tokens_per_owner: LookupMap::new(StorageKey::TokensPerOwner),
            token_approvals: LookupMap::new(StorageKey::TokenApprovals),
            authorized_consumers: IterableSet::new(StorageKey::AuthorizedConsumers),
        }
    }

    // =========================================================================
    // Agent Registration & Minting (ERC-8004)
    // =========================================================================

    pub fn mint_agent(
        &mut self,
        dna_hash: String,
        licenses: u64,
        license_price: U128,
        creator_fee: Option<u64>,
        cloneable: bool,
        uri: String,
        metadata: Option<Vec<MetadataEntryInput>>,
    ) -> u64 {
        self.assert_not_paused();
        let parsed_dna = hex_to_bytes32(&dna_hash).unwrap_or_else(|e| e.panic());
        man0_require!(!is_zero_hash(&parsed_dna), Man0Error::ZeroDnaHash);
        man0_require!(
            !self.dna_to_agent.contains_key(&parsed_dna),
            Man0Error::DnaAlreadyRegistered
        );
        validate_uri(&uri, true).unwrap_or_else(|e| e.panic());

        let caller = env::predecessor_account_id();
        self.internal_create_agent(
            parsed_dna,
            caller.clone(),
            caller,
            licenses,
            license_price.0,
            creator_fee.unwrap_or(1),
            cloneable,
            false,
            0,
            uri,
            metadata.unwrap_or_default(),
        )
    }

    pub fn register_agent(
        &mut self,
        uri: Option<String>,
        metadata: Option<Vec<MetadataEntryInput>>,
    ) -> u64 {
        self.assert_not_paused();
        let uri_str = uri.unwrap_or_default();
        validate_uri(&uri_str, true).unwrap_or_else(|e| e.panic());

        let caller = env::predecessor_account_id();
        let candidate_id = self.next_agent_id;
        let dna = derive_registration_dna(&caller, &uri_str, candidate_id);
        man0_require!(
            !self.dna_to_agent.contains_key(&dna),
            Man0Error::DnaAlreadyRegistered
        );

        self.internal_create_agent(
            dna,
            caller.clone(),
            caller,
            0,
            0,
            1,
            false,
            false,
            0,
            uri_str,
            metadata.unwrap_or_default(),
        )
    }

    // =========================================================================
    // Clone Engine
    // =========================================================================

    pub fn clone_agent(
        &mut self,
        original_agent_id: u64,
        params: CloneParams,
        uri: String,
    ) -> u64 {
        self.assert_not_paused();
        validate_uri(&uri, true).unwrap_or_else(|e| e.panic());

        let parent = self
            .agents
            .get(&original_agent_id)
            .unwrap_or_else(|| Man0Error::AgentNotFound.panic())
            .clone();

        man0_require!(!parent.is_clone, Man0Error::CannotCloneAClone);
        man0_require!(parent.cloneable, Man0Error::NotCloneable);

        let caller = env::predecessor_account_id();
        let clone_dna = derive_clone_dna(
            &parent.dna_hash,
            params.chain_id,
            &params.model,
            &caller,
            env::block_timestamp(),
        );
        man0_require!(
            !self.dna_to_agent.contains_key(&clone_dna),
            Man0Error::DnaAlreadyRegistered
        );

        let mut extra_meta = Vec::with_capacity(2);
        extra_meta.push(MetadataEntryInput {
            key: "chainId".to_string(),
            value: Base64VecU8(params.chain_id.to_string().into_bytes()),
        });
        extra_meta.push(MetadataEntryInput {
            key: "model".to_string(),
            value: Base64VecU8(params.model.clone().into_bytes()),
        });

        let cloned_agent_id = self.internal_create_agent(
            clone_dna,
            caller.clone(),
            caller.clone(),
            params.licenses,
            params.license_price.0,
            parent.creator_fee,
            false,
            true,
            original_agent_id,
            uri,
            extra_meta,
        );

        let mut list = self
            .clones_of
            .get(&original_agent_id)
            .cloned()
            .unwrap_or_default();
        list.push(cloned_agent_id);
        self.clones_of.insert(original_agent_id, list);
        self.total_clones = self.total_clones.saturating_add(1);

        Man0Event::AgentCloned {
            original_agent_id,
            cloned_agent_id,
            cloner: caller,
            chain_id: params.chain_id,
            model: params.model,
            license_price: params.license_price,
        }
        .emit();

        cloned_agent_id
    }

    // =========================================================================
    // Warp Engine
    // =========================================================================

    pub fn warp_agent(
        &mut self,
        original_hash: String,
        original_creator: Option<AccountId>,
        licenses: u64,
        license_price: U128,
        uri: String,
    ) -> u64 {
        self.assert_not_paused();
        validate_uri(&uri, true).unwrap_or_else(|e| e.panic());

        let parsed_orig = hex_to_bytes32(&original_hash).unwrap_or_else(|e| e.panic());
        man0_require!(!is_zero_hash(&parsed_orig), Man0Error::ZeroOriginalHash);
        man0_require!(
            !self.original_hash_to_agent.contains_key(&parsed_orig),
            Man0Error::AlreadyWarped
        );

        let warper = env::predecessor_account_id();
        let now = env::block_timestamp();
        let warp_dna = derive_warp_dna(&parsed_orig, &warper, now);
        man0_require!(
            !self.dna_to_agent.contains_key(&warp_dna),
            Man0Error::DnaAlreadyRegistered
        );

        let warped_agent_id = self.internal_create_agent(
            warp_dna,
            warper.clone(),
            warper.clone(),
            licenses,
            license_price.0,
            1,
            false,
            false,
            0,
            uri,
            Vec::new(),
        );

        let record = WarpRecord {
            warped_agent_id,
            original_hash: parsed_orig,
            original_creator: original_creator.clone(),
            warper: warper.clone(),
            warped_at: now,
            royalty_expiry: now.saturating_add(WARP_CLAIM_WINDOW_NS),
            royalties_claimed: false,
            accumulated_royalties: 0,
        };
        self.warp_records.insert(warped_agent_id, record);
        self.original_hash_to_agent
            .insert(parsed_orig, warped_agent_id);
        self.total_warped = self.total_warped.saturating_add(1);

        Man0Event::AgentWarped {
            warped_agent_id,
            original_hash: bytes32_to_hex(&parsed_orig),
            original_creator,
            warper,
            licenses,
            license_price,
        }
        .emit();

        warped_agent_id
    }
}
