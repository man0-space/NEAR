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
}
