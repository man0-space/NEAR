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

    pub fn reject_child(
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

        if child_contract == env::current_account_id() {
            if let Some(wf) = self.workflows.get(&child_id) {
                let creator = wf.creator.clone();
                self.direct_owners.insert(
                    child_id,
                    DirectOwner {
                        owner_id: creator,
                        parent_token_id: 0,
                        is_nft: false,
                    },
                );
            }
        }

        Man0Event::ChildRejected {
            parent_token_id: parent_id,
            child_contract,
            child_token_id: child_id,
        }
        .emit();
    }

    pub fn reject_all_children(&mut self, parent_id: u64, max_rejections: u64) {
        self.assert_not_paused();
        let root_owner = self.owner_of(parent_id);
        man0_require!(
            env::predecessor_account_id() == root_owner,
            Man0Error::Unauthorized
        );

        let pending = self
            .pending_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default();
        man0_require!(
            (pending.len() as u64) <= max_rejections,
            Man0Error::MaxPendingChildrenReached
        );
        self.pending_children.insert(parent_id, Vec::new());
    }

    #[allow(clippy::too_many_arguments)]
    pub fn unnest_child(
        &mut self,
        parent_id: u64,
        index: u64,
        child_contract: AccountId,
        child_id: u64,
        to: AccountId,
        is_nft: bool,
        dest_parent_id: Option<u64>,
    ) {
        self.assert_not_paused();
        let root_owner = self.owner_of(parent_id);
        man0_require!(
            env::predecessor_account_id() == root_owner,
            Man0Error::Unauthorized
        );

        let active = self
            .active_children
            .get_mut(&parent_id)
            .unwrap_or_else(|| Man0Error::ActiveChildMismatch.panic());
        let idx = index as usize;
        man0_require!(idx < active.len(), Man0Error::ActiveChildMismatch);
        let expected = Child {
            contract_id: child_contract.clone(),
            token_id: child_id,
        };
        man0_require!(active[idx] == expected, Man0Error::ActiveChildMismatch);
        active.remove(idx);

        if child_contract == env::current_account_id() {
            if is_nft {
                let target_parent = dest_parent_id.unwrap_or(0);
                man0_require!(target_parent > 0, Man0Error::WorkflowNotFound);
                self.direct_owners.insert(
                    child_id,
                    DirectOwner {
                        owner_id: to,
                        parent_token_id: target_parent,
                        is_nft: true,
                    },
                );
            } else {
                self.direct_owners.insert(
                    child_id,
                    DirectOwner {
                        owner_id: to.clone(),
                        parent_token_id: 0,
                        is_nft: false,
                    },
                );
                if let Some(wf) = self.workflows.get_mut(&child_id) {
                    wf.owner = to;
                }
            }
        }

        Man0Event::ChildUnnested {
            parent_token_id: parent_id,
            child_contract,
            child_token_id: child_id,
        }
        .emit();
    }

    pub fn set_max_pending_children(&mut self, token_id: u64, max_children: u16) {
        let root_owner = self.owner_of(token_id);
        man0_require!(
            env::predecessor_account_id() == root_owner,
            Man0Error::Unauthorized
        );
        self.max_pending_children.insert(token_id, max_children);
    }

    pub fn children_of(&self, parent_id: u64) -> Vec<Child> {
        self.active_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn pending_children_of(&self, parent_id: u64) -> Vec<Child> {
        self.pending_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn child_of(&self, parent_id: u64, index: u64) -> Option<Child> {
        self.active_children
            .get(&parent_id)
            .and_then(|v| v.get(index as usize).cloned())
    }

    pub fn pending_child_of(&self, parent_id: u64, index: u64) -> Option<Child> {
        self.pending_children
            .get(&parent_id)
            .and_then(|v| v.get(index as usize).cloned())
    }

    pub fn direct_owner_of(&self, token_id: u64) -> Option<DirectOwner> {
        self.direct_owners.get(&token_id).cloned()
    }

    pub fn owner_of(&self, token_id: u64) -> AccountId {
        let mut current = token_id;
        for _ in 0..MAX_NESTING_DEPTH {
            let direct = self
                .direct_owners
                .get(&current)
                .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic());
            if !direct.is_nft {
                return direct.owner_id.clone();
            }
            if direct.owner_id != env::current_account_id() {
                return direct.owner_id.clone();
            }
            current = direct.parent_token_id;
        }
        Man0Error::MaxNestingDepthExceeded.panic()
    }

    // =========================================================================
    // RFA (Request-For-Agent) Escrow Engine
    // =========================================================================

    pub fn create_rfa(
        &mut self,
        workflow_id: u64,
        title: String,
        description: String,
        required_skills: Vec<String>,
        offer_amount: U128,
    ) -> u64 {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();
        self.internal_create_rfa(
            caller,
            workflow_id,
            title,
            description,
            required_skills,
            offer_amount.0,
        )
    }

    pub fn submit_agent(&mut self, rfa_id: u64, agent_id: u64) {
        self.assert_not_paused();
        man0_require!(agent_id > 0, Man0Error::AgentNotFound);
        let rfa = self
            .rfas
            .get(&rfa_id)
            .unwrap_or_else(|| Man0Error::RfaNotFound.panic());
        man0_require!(rfa.status == RfaStatus::Open, Man0Error::RfaNotOpen);

        let caller = env::predecessor_account_id();
        if let Some(cached) = self.cached_agents.get(&agent_id) {
            man0_require!(caller == cached.creator, Man0Error::Unauthorized);
        }

        let mut subs = self
            .rfa_submissions
            .get(&rfa_id)
            .cloned()
            .unwrap_or_default();
        man0_require!(
            !subs.iter().any(|s| s.agent_id == agent_id),
            Man0Error::AgentAlreadySubmitted
        );
        subs.push(Submission {
            agent_id,
            creator: caller.clone(),
            submitted_at: U64(env::block_timestamp()),
        });
        self.rfa_submissions.insert(rfa_id, subs);

        Man0Event::AgentSubmittedToRfa {
            rfa_id,
            agent_id,
            creator: caller,
        }
        .emit();
    }

    pub fn accept_agent(&mut self, rfa_id: u64, agent_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();

        let subs = self
            .rfa_submissions
            .get(&rfa_id)
            .cloned()
            .unwrap_or_default();
        let sub = subs
            .iter()
            .find(|s| s.agent_id == agent_id)
            .cloned()
            .unwrap_or_else(|| Man0Error::AgentNotSubmitted.panic());

        let (workflow_id, offer_amount) = {
            let rfa = self
                .rfas
                .get_mut(&rfa_id)
                .unwrap_or_else(|| Man0Error::RfaNotFound.panic());
            man0_require!(caller == rfa.publisher, Man0Error::Unauthorized);
            man0_require!(rfa.status == RfaStatus::Open, Man0Error::RfaNotOpen);

            rfa.status = RfaStatus::Fulfilled;
            rfa.fulfilled_by_agent_id = agent_id;
            rfa.agent_creator = Some(sub.creator.clone());
            (rfa.workflow_id, rfa.offer_amount)
        };

        self.open_rfas.remove(&rfa_id);
        self.internal_resolve_workflow_rfa(workflow_id);

        if offer_amount > 0 {
            self.internal_payout(sub.creator.clone(), offer_amount, "RFA fulfillment payout");
        }

        Man0Event::RfaFulfilled {
            rfa_id,
            workflow_id,
            agent_id,
            agent_creator: sub.creator,
            offer_amount: U128(offer_amount),
        }
        .emit();
    }

    pub fn cancel_rfa(&mut self, rfa_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();

        let (workflow_id, offer_amount, publisher) = {
            let rfa = self
                .rfas
                .get_mut(&rfa_id)
                .unwrap_or_else(|| Man0Error::RfaNotFound.panic());
            man0_require!(caller == rfa.publisher, Man0Error::Unauthorized);
            man0_require!(rfa.status == RfaStatus::Open, Man0Error::RfaNotOpen);

            rfa.status = RfaStatus::Cancelled;
            (rfa.workflow_id, rfa.offer_amount, rfa.publisher.clone())
        };

        self.open_rfas.remove(&rfa_id);
        self.internal_resolve_workflow_rfa(workflow_id);

        if offer_amount > 0 {
            self.internal_payout(
                publisher.clone(),
                offer_amount,
                "RFA cancellation refund",
            );
        }

        Man0Event::RfaCancelled {
            rfa_id,
            workflow_id,
            publisher,
            refunded_amount: U128(offer_amount),
        }
        .emit();
    }

    // =========================================================================
    // Lease Engine
    // =========================================================================

    pub fn create_lease(&mut self, workflow_id: u64, duration_days: u64) -> u64 {
        self.assert_not_paused();
        man0_require!(!self.is_leased(workflow_id), Man0Error::WorkflowAlreadyLeased);

        if let Some(old_id) = self.active_lease_by_workflow.remove(&workflow_id) {
            if let Some(old_lease) = self.leases.get_mut(&old_id) {
                old_lease.is_active = false;
            }
            self.active_leases.remove(&old_id);
        }

        let wf = self
            .workflows
            .get(&workflow_id)
            .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic());
        man0_require!(wf.lease_enabled, Man0Error::LeaseNotEnabled);

        let effective_days = if duration_days == 0 {
            wf.lease_duration
        } else {
            duration_days
        };
        man0_require!(
            effective_days > 0 && effective_days <= wf.lease_duration,
            Man0Error::InvalidDuration
        );

        let creator_percent = wf.lease_percent;
        let leaser = env::predecessor_account_id();
        let start_time = env::block_timestamp();
        let duration_ns = effective_days
            .checked_mul(NANOS_PER_DAY)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        let end_time = start_time
            .checked_add(duration_ns)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());

        let lease_id = self.next_lease_id;
        self.next_lease_id = self
            .next_lease_id
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        self.total_leases = self.total_leases.saturating_add(1);

        let record = LeaseRecord {
            lease_id,
            workflow_id,
            leaser: leaser.clone(),
            start_time,
            end_time,
            creator_percent,
            is_active: true,
        };
        self.leases.insert(lease_id, record);
        self.active_lease_by_workflow.insert(workflow_id, lease_id);
        self.active_leases.insert(lease_id);

        let mut list = self
            .leases_by_leaser
            .get(&leaser)
            .cloned()
            .unwrap_or_default();
        list.push(lease_id);
        self.leases_by_leaser.insert(leaser.clone(), list);

        Man0Event::LeaseCreated {
            lease_id,
            workflow_id,
            leaser,
            start_time,
            end_time,
            creator_percent,
        }
        .emit();

        lease_id
    }

    pub fn terminate_lease(&mut self, lease_id: u64) {
        self.assert_not_paused();
        let caller = env::predecessor_account_id();

        let (workflow_id, leaser) = {
            let lease = self
                .leases
                .get(&lease_id)
                .unwrap_or_else(|| Man0Error::LeaseNotFound.panic());
            man0_require!(lease.is_active, Man0Error::LeaseNotActive);
            (lease.workflow_id, lease.leaser.clone())
        };

        let wf_creator = self
            .workflows
            .get(&workflow_id)
            .unwrap_or_else(|| Man0Error::WorkflowNotFound.panic())
            .creator
            .clone();
        man0_require!(
            caller == leaser || caller == wf_creator,
            Man0Error::Unauthorized
        );

        let lease_mut = self.leases.get_mut(&lease_id).unwrap();
        lease_mut.is_active = false;
        self.active_lease_by_workflow.remove(&workflow_id);
        self.active_leases.remove(&lease_id);

        Man0Event::LeaseTerminated {
            lease_id,
            workflow_id,
            terminated_by: caller,
        }
        .emit();
    }

    pub fn expire_lease_if_needed(&mut self, lease_id: u64) {
        let workflow_id = {
            let lease = self
                .leases
                .get_mut(&lease_id)
                .unwrap_or_else(|| Man0Error::LeaseNotFound.panic());
            man0_require!(lease.is_active, Man0Error::LeaseNotActive);
            man0_require!(
                env::block_timestamp() >= lease.end_time,
                Man0Error::LeaseNotExpired
            );
            lease.is_active = false;
            lease.workflow_id
        };
        self.active_lease_by_workflow.remove(&workflow_id);
        self.active_leases.remove(&lease_id);

        Man0Event::LeaseExpired {
            lease_id,
            workflow_id,
        }
        .emit();
    }

    pub fn distribute_lease_fees(&mut self, lease_id: u64, total_amount: U128) {
        self.assert_not_paused();
        self.internal_distribute_lease_fees(lease_id, total_amount.0);
    }

    pub fn calculate_fee_split(&self, lease_id: u64, total_amount: U128) -> (U128, U128) {
        let lease = self
            .leases
            .get(&lease_id)
            .unwrap_or_else(|| Man0Error::LeaseNotFound.panic());
        let creator_amount = total_amount
            .0
            .checked_mul(lease.creator_percent as u128)
            .unwrap()
            / 100;
        let leaser_amount = total_amount.0.saturating_sub(creator_amount);
        (U128(creator_amount), U128(leaser_amount))
    }

    // =========================================================================
    // NEP-141 `ft_on_transfer` Receiver (USDC Settlement)
    // =========================================================================

    /// NEP-141 standard FungibleTokenReceiver entrypoint.
    /// Returns unused token amount to refund to `sender_id`.
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.assert_not_paused();
        let token_contract = env::predecessor_account_id();
        man0_require!(
            self.payment_token.as_ref() == Some(&token_contract),
            Man0Error::InvalidPaymentToken
        );

        let action: WorkflowFtAction = serde_json::from_str(&msg)
            .unwrap_or_else(|_| Man0Error::InvalidPaymentMessage.panic());

        match action {
            WorkflowFtAction::MintWorkflow { params, agent_ids } => {
                let wf_id = self.internal_mint_workflow(
                    sender_id,
                    params,
                    agent_ids,
                    Some(amount.0),
                );
                let used = self.workflows.get(&wf_id).map(|w| w.total_price).unwrap_or(0);
                PromiseOrValue::Value(U128(amount.0.saturating_sub(used)))
            }
            WorkflowFtAction::AddAgentToWorkflow {
                workflow_id,
                agent_id,
            } => {
                let used = self.internal_add_agent_to_workflow(
                    sender_id,
                    workflow_id,
                    agent_id,
                    Some(amount.0),
                );
                PromiseOrValue::Value(U128(amount.0.saturating_sub(used)))
            }
            WorkflowFtAction::CreateRfa {
                workflow_id,
                title,
                description,
                required_skills,
            } => {
                let _ = self.internal_create_rfa(
                    sender_id,
                    workflow_id,
                    title,
                    description,
                    required_skills,
                    amount.0,
                );
                PromiseOrValue::Value(U128(0))
            }
            WorkflowFtAction::DistributeLeaseFees { lease_id } => {
                self.internal_distribute_lease_fees(lease_id, amount.0);
                PromiseOrValue::Value(U128(0))
            }
        }
    }

    // =========================================================================
    // NEP-171 / NEP-177 / NEP-178 / NEP-181 Standard NFT Implementation
    // =========================================================================

    pub fn nft_metadata(&self) -> NFTContractMetadata {
        NFTContractMetadata {
            spec: "nft-1.0.0".to_string(),
            name: "man0 Workflows".to_string(),
            symbol: "MAN0W".to_string(),
            icon: None,
            base_uri: None,
            reference: None,
            reference_hash: None,
        }
    }

    pub fn nft_token(&self, token_id: String) -> Option<JsonToken> {
        let id: u64 = token_id.parse().ok()?;
        let wf = self.workflows.get(&id)?;
        let approvals = self
            .token_approvals
            .get(&id)
            .cloned()
            .unwrap_or_default();
        Some(JsonToken {
            token_id,
            owner_id: self.owner_of(id),
            metadata: TokenMetadata {
                title: Some(wf.title.clone()),
                description: Some(wf.description.clone()),
                media: if wf.banner.is_empty() {
                    None
                } else {
                    Some(wf.banner.clone())
                },
                copies: Some(wf.units),
                issued_at: Some(wf.created_at.to_string()),
                reference: if wf.workflow_card_uri.is_empty() {
                    None
                } else {
                    Some(wf.workflow_card_uri.clone())
                },
                ..Default::default()
            },
            approved_account_ids: approvals,
        })
    }

    #[payable]
    pub fn nft_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
    ) {
        self.assert_not_paused();
        assert_at_least_one_yocto();
        let id = Self::parse_token_id(&token_id);
        let sender_id = env::predecessor_account_id();
        self.internal_transfer(&sender_id, &receiver_id, id, approval_id, memo);
    }

    #[payable]
    pub fn nft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        approval_id: Option<u64>,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<bool> {
        self.assert_not_paused();
        assert_at_least_one_yocto();
        let id = Self::parse_token_id(&token_id);
        let sender_id = env::predecessor_account_id();
        let (previous_owner, previous_approvals) =
            self.internal_transfer(&sender_id, &receiver_id, id, approval_id, memo);

        let promise = ext_nft_receiver::ext(receiver_id.clone())
            .with_static_gas(GAS_FOR_NFT_ON_TRANSFER)
            .nft_on_transfer(sender_id, previous_owner.clone(), token_id.clone(), msg)
            .then(
                ext_nft_resolver::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_RESOLVE_TRANSFER)
                    .nft_resolve_transfer(
                        previous_owner,
                        receiver_id,
                        token_id,
                        previous_approvals,
                        None,
                    ),
            );
        PromiseOrValue::Promise(promise)
    }

    #[private]
    pub fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
        _previous_agent_wallet: Option<String>,
    ) -> bool {
        let must_revert = match env::promise_result_checked(0, 64) {
            Ok(bytes) => serde_json::from_slice::<bool>(&bytes).unwrap_or(true),
            Err(_) => true,
        };

        if !must_revert {
            return true;
        }

        let id = Self::parse_token_id(&token_id);
        self.remove_token_from_owner(&receiver_id, id);
        self.add_token_to_owner(&previous_owner_id, id);

        if let Some(wf) = self.workflows.get_mut(&id) {
            wf.owner = previous_owner_id.clone();
        }
        self.direct_owners.insert(
            id,
            DirectOwner {
                owner_id: previous_owner_id.clone(),
                parent_token_id: 0,
                is_nft: false,
            },
        );
        if let Some(approvals) = approved_account_ids {
            self.token_approvals.insert(id, approvals);
        }

        Nep171Event::NftTransfer {
            authorized_id: None,
            old_owner_id: receiver_id,
            new_owner_id: previous_owner_id,
            token_ids: vec![token_id],
            memo: Some("revert".to_string()),
        }
        .emit();

        false
    }

    #[payable]
    pub fn nft_approve(
        &mut self,
        token_id: String,
        account_id: AccountId,
        msg: Option<String>,
    ) -> Option<Promise> {
        self.assert_not_paused();
        assert_at_least_one_yocto();
        let id = Self::parse_token_id(&token_id);
        let owner = self.owner_of(id);
        man0_require!(
            env::predecessor_account_id() == owner,
            Man0Error::Unauthorized
        );

        let approval_id = self.next_approval_id;
        self.next_approval_id = self.next_approval_id.saturating_add(1);

        let mut approvals = self
            .token_approvals
            .get(&id)
            .cloned()
            .unwrap_or_default();
        approvals.insert(account_id.clone(), approval_id);
        self.token_approvals.insert(id, approvals);

        msg.map(|m| {
            ext_nft_approval_receiver::ext(account_id)
                .with_static_gas(GAS_FOR_NFT_ON_APPROVE)
                .nft_on_approve(token_id, owner, approval_id, m)
        })
    }

    #[payable]
    pub fn nft_revoke(&mut self, token_id: String, account_id: AccountId) {
        self.assert_not_paused();
        assert_at_least_one_yocto();
        let id = Self::parse_token_id(&token_id);
        let owner = self.owner_of(id);
        man0_require!(
            env::predecessor_account_id() == owner,
            Man0Error::Unauthorized
        );
        if let Some(approvals) = self.token_approvals.get_mut(&id) {
            approvals.remove(&account_id);
        }
    }

    #[payable]
    pub fn nft_revoke_all(&mut self, token_id: String) {
        self.assert_not_paused();
        assert_at_least_one_yocto();
        let id = Self::parse_token_id(&token_id);
        let owner = self.owner_of(id);
        man0_require!(
            env::predecessor_account_id() == owner,
            Man0Error::Unauthorized
        );
        self.token_approvals.remove(&id);
    }

    pub fn nft_is_approved(
        &self,
        token_id: String,
        approved_account_id: AccountId,
        approval_id: Option<u64>,
    ) -> bool {
        let Ok(id) = token_id.parse::<u64>() else {
            return false;
        };
        let Some(approvals) = self.token_approvals.get(&id) else {
            return false;
        };
        let Some(&stored_id) = approvals.get(&approved_account_id) else {
            return false;
        };
        match approval_id {
            Some(expected) => stored_id == expected,
            None => true,
        }
    }

    pub fn nft_total_supply(&self) -> U128 {
        U128(self.total_workflows as u128)
    }

    pub fn nft_tokens(&self, from_index: Option<U128>, limit: Option<u64>) -> Vec<JsonToken> {
        let start = from_index.map(|i| i.0 as u64).unwrap_or(0);
        let max = limit.unwrap_or(50).min(100);
        let mut out = Vec::new();
        let mut id = start.saturating_add(1);
        while id < self.next_workflow_id && (out.len() as u64) < max {
            if let Some(tok) = self.nft_token(id.to_string()) {
                out.push(tok);
            }
            id += 1;
        }
        out
    }

    pub fn nft_supply_for_owner(&self, account_id: AccountId) -> U128 {
        self.tokens_per_owner
            .get(&account_id)
            .map(|s| U128(s.len() as u128))
            .unwrap_or(U128(0))
    }

    pub fn nft_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u64>,
    ) -> Vec<JsonToken> {
        let Some(set) = self.tokens_per_owner.get(&account_id) else {
            return Vec::new();
        };
        let start = from_index.map(|i| i.0 as usize).unwrap_or(0);
        let max = limit.unwrap_or(50).min(100) as usize;
        set.iter()
            .skip(start)
            .take(max)
            .filter_map(|id| self.nft_token(id.to_string()))
            .collect()
    }

    // =========================================================================
    // View Methods
    // =========================================================================

    pub fn get_workflow(&self, workflow_id: u64) -> Option<WorkflowView> {
        self.workflows.get(&workflow_id).map(|w| w.to_view())
    }

    pub fn get_workflow_agents(&self, workflow_id: u64) -> Vec<u64> {
        self.active_children
            .get(&workflow_id)
            .map(|children| {
                children
                    .iter()
                    .filter(|c| c.contract_id == self.factory)
                    .map(|c| c.token_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_agent_count(&self, workflow_id: u64) -> u64 {
        self.get_workflow_agents(workflow_id).len() as u64
    }

    pub fn is_complete(&self, workflow_id: u64) -> bool {
        self.workflows
            .get(&workflow_id)
            .map(|w| !w.has_active_rfa)
            .unwrap_or(false)
    }

    pub fn has_active_rfa(&self, workflow_id: u64) -> bool {
        self.workflows
            .get(&workflow_id)
            .map(|w| w.has_active_rfa)
            .unwrap_or(false)
    }

    pub fn get_workflows_by_creator(&self, creator: AccountId) -> Vec<u64> {
        self.workflows_by_creator
            .get(&creator)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_complete_workflows(&self) -> Vec<u64> {
        self.complete_workflows.iter().copied().collect()
    }

    pub fn get_workflows_with_rfa(&self) -> Vec<u64> {
        self.workflows_with_rfa.iter().copied().collect()
    }

    pub fn total_workflows(&self) -> u64 {
        self.total_workflows
    }

    pub fn get_rfa(&self, rfa_id: u64) -> Option<RfaView> {
        self.rfas.get(&rfa_id).map(|r| r.to_view())
    }

    pub fn get_rfa_for_workflow(&self, workflow_id: u64) -> u64 {
        self.workflows
            .get(&workflow_id)
            .map(|w| w.rfa_id)
            .unwrap_or(0)
    }

    pub fn get_submissions(&self, rfa_id: u64) -> Vec<Submission> {
        self.rfa_submissions
            .get(&rfa_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_open_rfas(&self) -> Vec<u64> {
        self.open_rfas.iter().copied().collect()
    }

    pub fn get_rfas_by_publisher(&self, publisher: AccountId) -> Vec<u64> {
        self.rfas_by_publisher
            .get(&publisher)
            .cloned()
            .unwrap_or_default()
    }

    pub fn total_rfas(&self) -> u64 {
        self.total_rfas
    }

    pub fn get_lease(&self, lease_id: u64) -> Option<LeaseView> {
        self.leases.get(&lease_id).map(|l| l.to_view())
    }

    pub fn get_active_lease_for_workflow(&self, workflow_id: u64) -> u64 {
        let Some(&lease_id) = self.active_lease_by_workflow.get(&workflow_id) else {
            return 0;
        };
        if let Some(lease) = self.leases.get(&lease_id) {
            if lease.is_active && env::block_timestamp() < lease.end_time {
                return lease_id;
            }
        }
        0
    }

    pub fn is_leased(&self, workflow_id: u64) -> bool {
        self.get_active_lease_for_workflow(workflow_id) > 0
    }

    pub fn get_leases_by_leaser(&self, leaser: AccountId) -> Vec<u64> {
        self.leases_by_leaser
            .get(&leaser)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_active_leases(&self) -> Vec<u64> {
        self.active_leases
            .iter()
            .copied()
            .filter(|id| {
                self.leases
                    .get(id)
                    .map(|l| l.is_active && env::block_timestamp() < l.end_time)
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn total_leases(&self) -> u64 {
        self.total_leases
    }

    // =========================================================================
    // Admin & Configuration
    // =========================================================================

    pub fn cache_agent_info(&mut self, agent_id: u64, info: AgentPermissionInfo) {
        let caller = env::predecessor_account_id();
        man0_require!(
            caller == self.admin || caller == self.factory,
            Man0Error::Unauthorized
        );
        self.cached_agents.insert(agent_id, info);
    }

    pub fn set_sync_factory_xcc(&mut self, enabled: bool) {
        self.assert_admin();
        self.sync_factory_xcc = enabled;
    }

    pub fn set_factory(&mut self, new_factory: AccountId) {
        self.assert_admin();
        self.factory = new_factory;
    }

    pub fn set_treasury(&mut self, new_treasury: AccountId) {
        self.assert_admin();
        self.treasury = new_treasury;
    }

    pub fn set_payment_token(&mut self, new_token: Option<AccountId>) {
        self.assert_admin();
        self.payment_token = new_token;
    }

    pub fn transfer_admin(&mut self, new_admin: AccountId) {
        self.assert_admin();
        self.admin = new_admin;
    }

    pub fn pause(&mut self) {
        self.assert_admin();
        self.paused = true;
    }

    pub fn unpause(&mut self) {
        self.assert_admin();
        self.paused = false;
    }

    pub fn get_admin(&self) -> AccountId {
        self.admin.clone()
    }

    pub fn get_treasury(&self) -> AccountId {
        self.treasury.clone()
    }

    pub fn get_factory(&self) -> AccountId {
        self.factory.clone()
    }

    pub fn get_payment_token(&self) -> Option<AccountId> {
        self.payment_token.clone()
    }

    // =========================================================================
    // Internal Helpers & Callbacks
    // =========================================================================

    fn assert_admin(&self) {
        man0_require!(
            env::predecessor_account_id() == self.admin,
            Man0Error::Unauthorized
        );
    }

    fn assert_not_paused(&self) {
        man0_require!(!self.paused, Man0Error::Paused);
    }

    fn parse_token_id(token_id: &str) -> u64 {
        token_id
            .parse::<u64>()
            .unwrap_or_else(|_| Man0Error::WorkflowNotFound.panic())
    }

    fn assert_no_nesting_cycle(&self, parent_id: u64, child_id: u64) {
        let mut current = parent_id;
        for _ in 0..MAX_NESTING_DEPTH {
            if current == child_id {
                Man0Error::NestingCycleDetected.panic();
            }
            let Some(direct) = self.direct_owners.get(&current) else {
                break;
            };
            if !direct.is_nft || direct.owner_id != env::current_account_id() {
                break;
            }
            current = direct.parent_token_id;
        }
    }

    fn internal_mint_workflow(
        &mut self,
        creator: AccountId,
        params: MintWorkflowParams,
        agent_ids: Vec<u64>,
        available_budget: Option<u128>,
    ) -> u64 {
        man0_require!(params.units > 0, Man0Error::ZeroUnits);
        man0_require!(
            params.lease_percent <= MAX_LEASE_PERCENT,
            Man0Error::InvalidLeasePercent
        );
        validate_uri(&params.workflow_card_uri, true).unwrap_or_else(|e| e.panic());

        let workflow_id = self.next_workflow_id;
        self.next_workflow_id = self
            .next_workflow_id
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        self.total_workflows = self.total_workflows.saturating_add(1);

        let record = WorkflowRecord {
            workflow_id,
            title: params.title.clone(),
            description: params.description,
            banner: params.banner,
            workflow_card_uri: params.workflow_card_uri,
            total_price: 0,
            units: params.units,
            units_minted: 0,
            creator: creator.clone(),
            owner: creator.clone(),
            lease_enabled: params.lease_enabled,
            lease_duration: params.lease_duration,
            lease_percent: params.lease_percent,
            has_coordinator: params.has_coordinator,
            coordinator_model: params.coordinator_model,
            has_active_rfa: false,
            rfa_id: 0,
            created_at: env::block_timestamp(),
        };
        self.workflows.insert(workflow_id, record);
        self.direct_owners.insert(
            workflow_id,
            DirectOwner {
                owner_id: creator.clone(),
                parent_token_id: 0,
                is_nft: false,
            },
        );
        self.active_children.insert(workflow_id, Vec::new());
        self.pending_children.insert(workflow_id, Vec::new());
        self.complete_workflows.insert(workflow_id);
        self.add_token_to_owner(&creator, workflow_id);

        let mut by_creator = self
            .workflows_by_creator
            .get(&creator)
            .cloned()
            .unwrap_or_default();
        by_creator.push(workflow_id);
        self.workflows_by_creator.insert(creator.clone(), by_creator);

        let mut remaining_budget = available_budget;
        for agent_id in agent_ids {
            let cost = self.internal_add_agent_to_workflow(
                creator.clone(),
                workflow_id,
                agent_id,
                remaining_budget,
            );
            if let Some(rem) = remaining_budget {
                remaining_budget = Some(rem.saturating_sub(cost));
            }
        }

        let total_price = self.workflows.get(&workflow_id).unwrap().total_price;

        Nep171Event::NftMint {
            owner_id: creator.clone(),
            token_ids: vec![workflow_id.to_string()],
            memo: None,
        }
        .emit();

        Man0Event::WorkflowMinted {
            workflow_id,
            creator,
            title: params.title,
            total_price: U128(total_price),
            units: params.units,
        }
        .emit();

        workflow_id
    }

    fn internal_add_agent_to_workflow(
        &mut self,
        caller: AccountId,
        workflow_id: u64,
        agent_id: u64,
        available_budget: Option<u128>,
    ) -> u128 {
        let root_owner = self.owner_of(workflow_id);
        man0_require!(caller == root_owner, Man0Error::Unauthorized);

        let candidate = Child {
            contract_id: self.factory.clone(),
            token_id: agent_id,
        };
        let mut children = self
            .active_children
            .get(&workflow_id)
            .cloned()
            .unwrap_or_default();
        man0_require!(
            !children.contains(&candidate),
            Man0Error::ChildAlreadyNested
        );

        let (price, agent_creator) = if let Some(cached) = self.cached_agents.get(&agent_id) {
            man0_require!(cached.exists, Man0Error::AgentNotFound);
            (cached.license_price.0, cached.creator.clone())
        } else {
            (0u128, self.treasury.clone())
        };

        if let Some(budget) = available_budget {
            man0_require!(budget >= price, Man0Error::InsufficientDeposit);
        }

        children.push(candidate);
        self.active_children.insert(workflow_id, children);
        self.agent_license_prices.insert(
            WorkflowAgentPair {
                workflow_id,
                agent_id,
            },
            price,
        );

        let wf = self.workflows.get_mut(&workflow_id).unwrap();
        wf.total_price = wf
            .total_price
            .checked_add(price)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());

        if price > 0 {
            let treasury_fee = price.checked_mul(TREASURY_FEE_BPS).unwrap() / BPS_DENOMINATOR;
            let creator_share = price.saturating_sub(treasury_fee);
            if treasury_fee > 0 {
                self.internal_payout(
                    self.treasury.clone(),
                    treasury_fee,
                    "Workflow license treasury fee",
                );
            }
            if creator_share > 0 {
                self.internal_payout(
                    agent_creator,
                    creator_share,
                    "Workflow license creator payout",
                );
            }
        }

        if self.sync_factory_xcc {
            let _ = ext_factory::ext(self.factory.clone())
                .with_static_gas(GAS_FOR_CONSUME_LICENSE)
                .consume_license(agent_id, env::current_account_id(), workflow_id)
                .then(
                    Self::ext(env::current_account_id())
                        .with_static_gas(GAS_FOR_CALLBACK)
                        .on_consume_license(workflow_id, agent_id, U128(price)),
                );
        }

        Man0Event::AgentAddedToWorkflow {
            workflow_id,
            agent_id,
            license_price: U128(price),
        }
        .emit();

        price
    }

    #[private]
    pub fn on_consume_license(&mut self, workflow_id: u64, agent_id: u64, previous_price: U128) {
        match env::promise_result_checked(0, 4096) {
            Ok(bytes) => {
                if let Ok(info) = serde_json::from_slice::<AgentPermissionInfo>(&bytes) {
                    self.cached_agents.insert(agent_id, info.clone());
                    let actual_price = info.license_price.0;
                    if actual_price != previous_price.0 {
                        self.agent_license_prices.insert(
                            WorkflowAgentPair {
                                workflow_id,
                                agent_id,
                            },
                            actual_price,
                        );
                        if let Some(wf) = self.workflows.get_mut(&workflow_id) {
                            wf.total_price = wf
                                .total_price
                                .saturating_sub(previous_price.0)
                                .saturating_add(actual_price);
                        }
                    }
                }
            }
            Err(_) => {
                // Rollback child addition if factory license consumption failed
                if let Some(children) = self.active_children.get_mut(&workflow_id) {
                    children.retain(|c| !(c.contract_id == self.factory && c.token_id == agent_id));
                }
                self.agent_license_prices.remove(&WorkflowAgentPair {
                    workflow_id,
                    agent_id,
                });
                if let Some(wf) = self.workflows.get_mut(&workflow_id) {
                    wf.total_price = wf.total_price.saturating_sub(previous_price.0);
                }
            }
        }
    }
}
