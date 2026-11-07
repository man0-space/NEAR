use man0::error::Man0Error;
use man0::events::{Man0Event, Nep171Event};
use man0::ext::{
    ext_factory, ext_ft, ext_nft_approval_receiver, ext_nft_receiver, ext_nft_resolver,
    WorkflowFtAction,
};
use man0::man0_require;
use man0::metadata::validate_uri;
use man0::state::{
    AgentPermissionInfo, Child, DirectOwner, JsonToken, LeaseRecord, LeaseView,
    MintWorkflowParams, NFTContractMetadata, RfaRecord, RfaStatus, RfaView, Submission,
    TokenMetadata, WorkflowRecord, WorkflowView, BPS_DENOMINATOR, DEFAULT_MAX_PENDING_CHILDREN,
    MAX_LEASE_PERCENT, MAX_NESTING_DEPTH, NANOS_PER_DAY, TREASURY_FEE_BPS,
};
use man0::storage::assert_at_least_one_yocto;
use near_sdk::json_types::{U128, U64};
use near_sdk::store::{IterableSet, LookupMap};
use near_sdk::{
    env, near, AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};
use std::collections::HashMap;

const GAS_FOR_CONSUME_LICENSE: Gas = Gas::from_tgas(12);
const GAS_FOR_REVOKE_LICENSE: Gas = Gas::from_tgas(10);
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(15);
const GAS_FOR_NFT_ON_TRANSFER: Gas = Gas::from_tgas(25);
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(15);
const GAS_FOR_NFT_ON_APPROVE: Gas = Gas::from_tgas(15);
const GAS_FOR_CALLBACK: Gas = Gas::from_tgas(20);

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Workflows,
    ActiveChildren,
    PendingChildren,
    MaxPendingChildren,
    DirectOwners,
    AgentLicensePrices,
    WorkflowsByCreator,
    CompleteWorkflows,
    WorkflowsWithRfa,
    TokensPerOwner,
    OwnerSet { account_hash: [u8; 32] },
    TokenApprovals,
    Rfas,
    RfaSubmissions,
    OpenRfas,
    RfasByPublisher,
    Leases,
    ActiveLeaseByWorkflow,
    LeasesByLeaser,
    ActiveLeases,
    CachedAgents,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct WorkflowAgentPair {
    workflow_id: u64,
    agent_id: u64,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct WorkflowContract {
    admin: AccountId,
    treasury: AccountId,
    factory: AccountId,
    payment_token: Option<AccountId>,
    paused: bool,
    sync_factory_xcc: bool,
    next_workflow_id: u64,
    total_workflows: u64,
    next_rfa_id: u64,
    total_rfas: u64,
    next_lease_id: u64,
    total_leases: u64,
    next_approval_id: u64,
    workflows: LookupMap<u64, WorkflowRecord>,
    active_children: LookupMap<u64, Vec<Child>>,
    pending_children: LookupMap<u64, Vec<Child>>,
    max_pending_children: LookupMap<u64, u16>,
    direct_owners: LookupMap<u64, DirectOwner>,
    agent_license_prices: LookupMap<WorkflowAgentPair, u128>,
    workflows_by_creator: LookupMap<AccountId, Vec<u64>>,
    complete_workflows: IterableSet<u64>,
    workflows_with_rfa: IterableSet<u64>,
    tokens_per_owner: LookupMap<AccountId, IterableSet<u64>>,
    token_approvals: LookupMap<u64, HashMap<AccountId, u64>>,
    rfas: LookupMap<u64, RfaRecord>,
    rfa_submissions: LookupMap<u64, Vec<Submission>>,
    open_rfas: IterableSet<u64>,
    rfas_by_publisher: LookupMap<AccountId, Vec<u64>>,
    leases: LookupMap<u64, LeaseRecord>,
    active_lease_by_workflow: LookupMap<u64, u64>,
    leases_by_leaser: LookupMap<AccountId, Vec<u64>>,
    active_leases: IterableSet<u64>,
    cached_agents: LookupMap<u64, AgentPermissionInfo>,
}

#[near]
impl WorkflowContract {
    #[init]
    pub fn new(
        admin: AccountId,
        treasury: AccountId,
        factory: AccountId,
        payment_token: Option<AccountId>,
    ) -> Self {
        Self {
            admin,
            treasury,
            factory,
            payment_token,
            paused: false,
            sync_factory_xcc: true,
            next_workflow_id: 1,
            total_workflows: 0,
            next_rfa_id: 1,
            total_rfas: 0,
            next_lease_id: 1,
            total_leases: 0,
            next_approval_id: 1,
            workflows: LookupMap::new(StorageKey::Workflows),
            active_children: LookupMap::new(StorageKey::ActiveChildren),
            pending_children: LookupMap::new(StorageKey::PendingChildren),
            max_pending_children: LookupMap::new(StorageKey::MaxPendingChildren),
            direct_owners: LookupMap::new(StorageKey::DirectOwners),
            agent_license_prices: LookupMap::new(StorageKey::AgentLicensePrices),
            workflows_by_creator: LookupMap::new(StorageKey::WorkflowsByCreator),
            complete_workflows: IterableSet::new(StorageKey::CompleteWorkflows),
            workflows_with_rfa: IterableSet::new(StorageKey::WorkflowsWithRfa),
            tokens_per_owner: LookupMap::new(StorageKey::TokensPerOwner),
            token_approvals: LookupMap::new(StorageKey::TokenApprovals),
            rfas: LookupMap::new(StorageKey::Rfas),
            rfa_submissions: LookupMap::new(StorageKey::RfaSubmissions),
            open_rfas: IterableSet::new(StorageKey::OpenRfas),
            rfas_by_publisher: LookupMap::new(StorageKey::RfasByPublisher),
            leases: LookupMap::new(StorageKey::Leases),
            active_lease_by_workflow: LookupMap::new(StorageKey::ActiveLeaseByWorkflow),
            leases_by_leaser: LookupMap::new(StorageKey::LeasesByLeaser),
            active_leases: IterableSet::new(StorageKey::ActiveLeases),
            cached_agents: LookupMap::new(StorageKey::CachedAgents),
        }
    }

    // =========================================================================
    // Workflow Minting & Composition (ERC-7401)
    // =========================================================================

    pub fn mint_workflow(&mut self, params: MintWorkflowParams, agent_ids: Vec<u64>) -> u64 {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        self.internal_mint_workflow(caller, params, agent_ids, None)
    }

    pub fn add_agent_to_workflow(&mut self, workflow_id: u64, agent_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        let _ = self.internal_add_agent_to_workflow(caller, workflow_id, agent_id, None);
    }

    pub fn remove_agent_from_workflow(&mut self, workflow_id: u64, agent_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        let root_owner = self.owner_of(workflow_id);
        man0_require!(caller == root_owner, Man0Error::Unauthorized);

        let children = self
            .active_children
            .get_mut(&workflow_id)
            .unwrap_or_else(|| Man0Error::ActiveChildMismatch.panic());
        let pos = children
            .iter()
            .position(|c| c.contract_id == self.factory && c.token_id == agent_id)
            .unwrap_or_else(|| Man0Error::ActiveChildMismatch.panic());
        children.remove(pos);

        let pair = WorkflowAgentPair {
            workflow_id,
            agent_id,
        };
        let price = self.agent_license_prices.remove(&pair).unwrap_or(0);

        let wf = self.workflows.get_mut(&workflow_id).unwrap();
        wf.total_price = wf.total_price.saturating_sub(price);

        if self.sync_factory_xcc {
            let _ = ext_factory::ext(self.factory.clone())
                .with_static_gas(GAS_FOR_REVOKE_LICENSE)
                .revoke_license(agent_id, env::current_account_id(), workflow_id);
        }

        Man0Event::AgentRemovedFromWorkflow {
            workflow_id,
            agent_id,
            license_price: U128(price),
        }
        .emit();
    }

    pub fn set_coordinator(
        &mut self,
        workflow_id: u64,
        has_coordinator: bool,
        model: String,
    ) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        let root_owner = self.owner_of(workflow_id);
        man0_require!(caller == root_owner, Man0Error::Unauthorized);

        let wf = self
            .workflows
            .get_mut(&workflow_id)
            .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic());
        wf.has_coordinator = has_coordinator;
        wf.coordinator_model = model;
    }

    pub fn update_lease_settings(
        &mut self,
        workflow_id: u64,
        enabled: bool,
        duration_days: u64,
        percent: u8,
    ) {
        self.assert_not_paused();
        man0_require!(percent <= MAX_LEASE_PERCENT, Man0Error::InvalidLeasePercent);
        let caller = env::predecessor_account_id();
        let root_owner = self.owner_of(workflow_id);
        man0_require!(caller == root_owner, Man0Error::Unauthorized);

        let wf = self
            .workflows
            .get_mut(&workflow_id)
            .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic());
        wf.lease_enabled = enabled;
        wf.lease_duration = duration_days;
        wf.lease_percent = percent;
    }

    pub fn consume_unit(&mut self, workflow_id: u64) -> u64 {
        self.assert_not_paused();
        let wf = self
            .workflows
            .get_mut(&workflow_id)
            .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic());
        if wf.units > 0 {
            man0_require!(
                wf.units_minted < wf.units,
                Man0Error::WorkflowUnitsExhausted
            );
        }
        wf.units_minted = wf
            .units_minted
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        wf.units_minted
    }

    // =========================================================================
    // ERC-7401 Nestable NFT Operations
    // =========================================================================

    pub fn propose_child(
        &mut self,
        parent_id: u64,
        child_contract: AccountId,
        child_id: u64,
    ) {
        self.assert_not_paused();
        man0_require!(
            self.workflows.contains_key(&parent_id),
            Man0Error::WorkflowNotFound
        );
        if child_contract == env::current_account_id() {
            man0_require!(parent_id != child_id, Man0Error::NestingCycleDetected);
            self.assert_no_nesting_cycle(parent_id, child_id);
            let child_root = self.owner_of(child_id);
            man0_require!(
                env::predecessor_account_id() == child_root,
                Man0Error::Unauthorized
            );
            self.direct_owners.insert(
                child_id,
                DirectOwner {
                    owner_id: env::current_account_id(),
                    parent_token_id: parent_id,
                    is_nft: true,
                },
            );
        }

        let max_pending = self
            .max_pending_children
            .get(&parent_id)
            .copied()
            .unwrap_or(DEFAULT_MAX_PENDING_CHILDREN) as usize;
        let mut pending = self
            .pending_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default();
        man0_require!(
            pending.len() < max_pending,
            Man0Error::MaxPendingChildrenReached
        );

        let candidate = Child {
            contract_id: child_contract.clone(),
            token_id: child_id,
        };
        man0_require!(
            !pending.contains(&candidate),
            Man0Error::ChildAlreadyProposed
        );
        let active = self
            .active_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default();
        man0_require!(!active.contains(&candidate), Man0Error::ChildAlreadyNested);

        pending.push(candidate);
        self.pending_children.insert(parent_id, pending);

        Man0Event::ChildProposed {
            parent_token_id: parent_id,
            child_contract,
            child_token_id: child_id,
        }
        .emit();
    }

    pub fn accept_child(
        &mut self,
        parent_id: u64,
        index: u64,
        child_contract: AccountId,
        child_id: u64,
    ) {
        self.assert_not_paused();
        let root_owner = self.owner_of(parent_id);
        man0_require!(
            env::predecessor_account_id() == root_owner,
            Man0Error::Unauthorized
        );

        let pending = self
            .pending_children
            .get_mut(&parent_id)
            .unwrap_or_else(|| Man0Error::PendingChildMismatch.panic());
        let idx = index as usize;
        man0_require!(idx < pending.len(), Man0Error::PendingChildMismatch);
        let expected = Child {
            contract_id: child_contract.clone(),
            token_id: child_id,
        };
        man0_require!(pending[idx] == expected, Man0Error::PendingChildMismatch);
        pending.remove(idx);

        let mut active = self
            .active_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default();
        active.push(expected);
        self.active_children.insert(parent_id, active);

        Man0Event::ChildAccepted {
            parent_token_id: parent_id,
            child_contract,
            child_token_id: child_id,
        }
        .emit();
    }
}
