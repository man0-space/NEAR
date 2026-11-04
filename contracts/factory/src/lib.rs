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

    pub fn record_unclaimed_warp_royalty(&mut self, warped_agent_id: u64, amount: U128) {
        self.assert_authorized_consumer_or_admin();
        let record = self
            .warp_records
            .get_mut(&warped_agent_id)
            .unwrap_or_else(|| Man0Error::NotWarpedAgent.panic());
        if record.original_creator.is_none()
            && !record.royalties_claimed
            && env::block_timestamp() <= record.royalty_expiry
        {
            record.accumulated_royalties = record
                .accumulated_royalties
                .checked_add(amount.0)
                .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        }
    }

    pub fn transfer_unclaimed_royalties(
        &mut self,
        warped_agent_id: u64,
        verified_creator: AccountId,
    ) {
        man0_require!(
            env::predecessor_account_id() == self.treasury,
            Man0Error::Unauthorized
        );
        let record = self
            .warp_records
            .get_mut(&warped_agent_id)
            .unwrap_or_else(|| Man0Error::NotWarpedAgent.panic());

        man0_require!(
            record.original_creator.is_none(),
            Man0Error::CreatorAlreadyKnown
        );
        man0_require!(
            !record.royalties_claimed,
            Man0Error::RoyaltiesAlreadyClaimed
        );
        man0_require!(
            env::block_timestamp() <= record.royalty_expiry,
            Man0Error::ClaimPeriodExpired
        );

        let amount = record.accumulated_royalties;
        record.original_creator = Some(verified_creator.clone());
        record.royalties_claimed = true;
        record.accumulated_royalties = 0;

        if amount > 0 {
            if let Some(ref ft) = self.payment_token {
                let _ = ext_ft::ext(ft.clone())
                    .with_attached_deposit(NearToken::from_yoctonear(1))
                    .with_static_gas(GAS_FOR_FT_TRANSFER)
                    .ft_transfer(
                        verified_creator.clone(),
                        U128(amount),
                        Some("Unclaimed warp royalties".to_string()),
                    );
            }
        }

        Man0Event::UnclaimedWarpRoyaltiesTransferred {
            warped_agent_id,
            verified_creator,
            amount: U128(amount),
        }
        .emit();
    }

    pub fn get_warped_creator_split(
        &self,
        warped_agent_id: u64,
        total_amount: U128,
    ) -> WarpSplitView {
        let record = self
            .warp_records
            .get(&warped_agent_id)
            .unwrap_or_else(|| Man0Error::NotWarpedAgent.panic());

        let total = total_amount.0;
        let warper_amount = total
            .checked_mul(WARP_WARPER_SHARE_BPS)
            .unwrap()
            / BPS_DENOMINATOR;
        let creator_share = total
            .checked_mul(WARP_CREATOR_SHARE_BPS)
            .unwrap()
            / BPS_DENOMINATOR;
        let base_treasury = total
            .saturating_sub(warper_amount)
            .saturating_sub(creator_share);

        if let Some(ref creator) = record.original_creator {
            WarpSplitView {
                warper: record.warper.clone(),
                warper_amount: U128(warper_amount),
                creator: Some(creator.clone()),
                creator_amount: U128(creator_share),
                treasury: self.treasury.clone(),
                treasury_amount: U128(base_treasury),
                unclaimed_creator_share: U128(0),
            }
        } else {
            let unclaimed = if !record.royalties_claimed
                && env::block_timestamp() <= record.royalty_expiry
            {
                creator_share
            } else {
                0
            };
            WarpSplitView {
                warper: record.warper.clone(),
                warper_amount: U128(warper_amount),
                creator: None,
                creator_amount: U128(0),
                treasury: self.treasury.clone(),
                treasury_amount: U128(base_treasury.saturating_add(creator_share)),
                unclaimed_creator_share: U128(unclaimed),
            }
        }
    }

    // =========================================================================
    // License Management
    // =========================================================================

    pub fn consume_license(
        &mut self,
        agent_id: u64,
        workflow_contract: AccountId,
        workflow_id: u64,
    ) -> AgentPermissionInfo {
        self.assert_not_paused();
        self.assert_authorized_consumer_or_admin();

        let key = LicenseKey {
            agent_id,
            workflow_contract: workflow_contract.clone(),
            workflow_id,
        };
        if let Some(existing) = self.licenses.get(&key) {
            man0_require!(!existing.is_active, Man0Error::AlreadyLicensed);
        }

        let agent = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| Man0Error::AgentNotFound.panic());

        if agent.licenses > 0 {
            man0_require!(
                agent.licenses_minted < agent.licenses,
                Man0Error::NoLicensesAvailable
            );
        }

        agent.licenses_minted = agent
            .licenses_minted
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());

        let price = agent.license_price;
        let owner = agent.owner.clone();
        let creator = agent.creator.clone();
        let wallet = agent.agent_wallet.clone().unwrap_or_default();

        self.licenses.insert(
            key,
            LicenseRecord {
                agent_id,
                workflow_contract: workflow_contract.clone(),
                workflow_id,
                licensed_at: U64(env::block_timestamp()),
                is_active: true,
                price_paid: U128(price),
            },
        );

        let warp_info = self.warp_records.get(&agent_id);
        let is_warped = warp_info.is_some();
        let warp_creator = warp_info.and_then(|w| w.original_creator.clone());

        Man0Event::LicenseConsumed {
            agent_id,
            workflow_contract,
            workflow_id,
            price_paid: U128(price),
        }
        .emit();

        AgentPermissionInfo {
            exists: true,
            owner,
            creator,
            agent_wallet: wallet,
            is_approved_operator: false,
            license_price: U128(price),
            is_warped,
            warp_creator,
        }
    }

    pub fn revoke_license(
        &mut self,
        agent_id: u64,
        workflow_contract: AccountId,
        workflow_id: u64,
    ) {
        self.assert_not_paused();
        self.assert_authorized_consumer_or_admin();

        let key = LicenseKey {
            agent_id,
            workflow_contract: workflow_contract.clone(),
            workflow_id,
        };
        let record = self
            .licenses
            .get_mut(&key)
            .unwrap_or_else(|| Man0Error::LicenseNotFound.panic());
        man0_require!(record.is_active, Man0Error::LicenseNotFound);
        record.is_active = false;

        let agent = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| Man0Error::AgentNotFound.panic());
        if agent.licenses_minted > 0 {
            agent.licenses_minted -= 1;
        }

        Man0Event::LicenseRevoked {
            agent_id,
            workflow_contract,
            workflow_id,
        }
        .emit();
    }

    pub fn update_price(&mut self, agent_id: u64, new_price: U128) {
        self.assert_not_paused();
        let agent = self
            .agents
            .get_mut(&agent_id)
            .unwrap_or_else(|| Man0Error::AgentNotFound.panic());
        man0_require!(
            env::predecessor_account_id() == agent.creator,
            Man0Error::Unauthorized
        );
        let old_price = agent.license_price;
        agent.license_price = new_price.0;

        Man0Event::PriceUpdated {
            agent_id,
            old_price: U128(old_price),
            new_price,
        }
        .emit();
    }

    // =========================================================================
    // ERC-8004 Wallet & Metadata Management
    // =========================================================================

    pub fn set_agent_wallet(
        &mut self,
        agent_id: u64,
        new_wallet: String,
        deadline: Option<U64>,
        signer_public_key: Option<Base64VecU8>,
        signature: Option<Base64VecU8>,
    ) {
        self.assert_not_paused();
        man0_require!(!new_wallet.trim().is_empty(), Man0Error::Unauthorized);

        let caller = env::predecessor_account_id();
        self.assert_owner_or_approved(agent_id, &caller);

        let owner = self.agents.get(&agent_id).unwrap().owner.clone();

        if let Some(dl) = deadline {
            man0_require!(
                env::block_timestamp() <= dl.0,
                Man0Error::SignatureExpired
            );
        }

        man0_require!(
            signer_public_key.is_some() == signature.is_some(),
            Man0Error::InvalidSignature
        );

        if let (Some(pk_bytes), Some(sig_bytes)) = (signer_public_key, signature) {
            let dl_ns = deadline.map(|d| d.0).unwrap_or(u64::MAX);
            man0_require!(
                env::block_timestamp() <= dl_ns,
                Man0Error::SignatureExpired
            );
            let pk_arr: [u8; 32] = pk_bytes
                .0
                .try_into()
                .unwrap_or_else(|_| Man0Error::InvalidSignature.panic());
            let sig_arr: [u8; 64] = sig_bytes
                .0
                .try_into()
                .unwrap_or_else(|_| Man0Error::InvalidSignature.panic());
            let msg_hash = derive_wallet_signing_hash(
                &env::current_account_id(),
                agent_id,
                &owner,
                &new_wallet,
                dl_ns,
            );
            man0_require!(
                env::ed25519_verify(&sig_arr, &msg_hash, &pk_arr),
                Man0Error::InvalidSignature
            );
        }

        let agent = self.agents.get_mut(&agent_id).unwrap();
        agent.agent_wallet = Some(new_wallet.clone());

        self.internal_set_metadata_raw(
            agent_id,
            RESERVED_KEY_AGENT_WALLET.to_string(),
            new_wallet.as_bytes().to_vec(),
        );

        Man0Event::AgentWalletUpdated {
            agent_id,
            new_wallet,
        }
        .emit();
    }

    pub fn clear_agent_wallet(&mut self, agent_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        self.assert_owner_or_approved(agent_id, &caller);

        let agent = self.agents.get_mut(&agent_id).unwrap();
        agent.agent_wallet = None;

        self.internal_set_metadata_raw(
            agent_id,
            RESERVED_KEY_AGENT_WALLET.to_string(),
            Vec::new(),
        );

        Man0Event::AgentWalletUpdated {
            agent_id,
            new_wallet: String::new(),
        }
        .emit();
    }

    pub fn set_metadata(&mut self, agent_id: u64, key: String, value: Base64VecU8) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        self.assert_owner_or_approved(agent_id, &caller);
        validate_user_metadata_entry(&key, &value.0).unwrap_or_else(|e| e.panic());

        self.internal_set_metadata_raw(agent_id, key.clone(), value.0);
        Man0Event::MetadataUpdated { agent_id, key }.emit();
    }

    pub fn remove_metadata(&mut self, agent_id: u64, key: String) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        self.assert_owner_or_approved(agent_id, &caller);
        man0_require!(
            key != RESERVED_KEY_AGENT_WALLET,
            Man0Error::ReservedMetadataKey
        );

        let meta_key = AgentMetadataKey {
            agent_id,
            key: key.clone(),
        };
        man0_require!(
            self.agent_metadata.remove(&meta_key).is_some(),
            Man0Error::MetadataNotFound
        );
        if let Some(keys) = self.agent_metadata_keys.get_mut(&agent_id) {
            keys.retain(|k| k != &key);
        }
        Man0Event::MetadataUpdated { agent_id, key }.emit();
    }

    // =========================================================================
    // NEP-171 / NEP-177 / NEP-178 / NEP-181 Standard NFT Implementation
    // =========================================================================

    pub fn nft_metadata(&self) -> NFTContractMetadata {
        NFTContractMetadata {
            spec: "nft-1.0.0".to_string(),
            name: "man0 Agents".to_string(),
            symbol: "MAN0A".to_string(),
            icon: None,
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    pub fn nft_token(&self, token_id: String) -> Option<JsonToken> {
        let id: u64 = token_id.parse().ok()?;
        let agent = self.agents.get(&id)?;
        let approvals = self
            .token_approvals
            .get(&id)
            .cloned()
            .unwrap_or_default();
        Some(JsonToken {
            token_id,
            owner_id: agent.owner.clone(),
            metadata: TokenMetadata {
                title: Some(format!("man0 Agent #{}", id)),
                description: Some(bytes32_to_hex(&agent.dna_hash)),
                media: if agent.uri.is_empty() {
                    None
                } else {
                    Some(agent.uri.clone())
                },
                copies: if agent.licenses == 0 {
                    None
                } else {
                    Some(agent.licenses)
                },
                issued_at: Some(agent.created_at.to_string()),
                reference: if agent.uri.is_empty() {
                    None
                } else {
                    Some(agent.uri.clone())
                },
                ..Default::default()
            },
            approved_account_ids: approvals,
        })
    }
}
