use man0::error::Man0Error;
use man0::events::Man0Event;
use man0::ext::ext_factory;
use man0::fixed::{aggregate_summary, validate_feedback_value};
use man0::hash::hex_to_bytes32;
use man0::man0_require;
use man0::metadata::validate_uri;
use man0::state::{
    AgentPermissionInfo, FeedbackKey, FeedbackRecord, FeedbackView, SummaryResult,
};
use near_sdk::json_types::I128;
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, AccountId, BorshStorageKey, Gas, PanicOnDefault, PromiseOrValue,
};

const GAS_FOR_GET_PERMISSIONS: Gas = Gas::from_tgas(10);
const GAS_FOR_ON_GIVE_FEEDBACK: Gas = Gas::from_tgas(20);

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Feedback,
    LastIndex,
    Clients,
    ResponseCounts,
    Responders,
    CachedAgents,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct AgentClientPair {
    agent_id: u64,
    client: AccountId,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ResponseCountKey {
    agent_id: u64,
    client: AccountId,
    feedback_index: u64,
    responder: AccountId,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct ReputationContract {
    admin: AccountId,
    factory: AccountId,
    verify_via_xcc: bool,
    feedback: LookupMap<FeedbackKey, FeedbackRecord>,
    last_index: LookupMap<AgentClientPair, u64>,
    clients: LookupMap<u64, Vec<AccountId>>,
    response_counts: LookupMap<ResponseCountKey, u64>,
    responders: LookupMap<FeedbackKey, Vec<AccountId>>,
    cached_agents: LookupMap<u64, AgentPermissionInfo>,
}

#[near]
impl ReputationContract {
    #[init]
    pub fn new(admin: AccountId, factory: AccountId) -> Self {
        Self {
            admin,
            factory,
            verify_via_xcc: true,
            feedback: LookupMap::new(StorageKey::Feedback),
            last_index: LookupMap::new(StorageKey::LastIndex),
            clients: LookupMap::new(StorageKey::Clients),
            response_counts: LookupMap::new(StorageKey::ResponseCounts),
            responders: LookupMap::new(StorageKey::Responders),
            cached_agents: LookupMap::new(StorageKey::CachedAgents),
        }
    }
}
