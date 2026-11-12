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

    pub fn give_feedback(
        &mut self,
        agent_id: u64,
        value: I128,
        value_decimals: u8,
        tag1: String,
        tag2: String,
        endpoint: String,
        feedback_uri: String,
        feedback_hash: String,
    ) -> PromiseOrValue<u64> {
        man0_require!(agent_id > 0, Man0Error::AgentNotFound);
        validate_feedback_value(value.0, value_decimals).unwrap_or_else(|e| e.panic());
        validate_uri(&feedback_uri, true).unwrap_or_else(|e| e.panic());
        let parsed_hash = if feedback_hash.is_empty() {
            [0u8; 32]
        } else {
            hex_to_bytes32(&feedback_hash).unwrap_or_else(|e| e.panic())
        };

        let caller = env::predecessor_account_id();

        if !self.verify_via_xcc {
            let info = self
                .cached_agents
                .get(&agent_id)
                .cloned()
                .unwrap_or_else(|| Man0Error::AgentNotFound.panic());
            Self::assert_not_self_feedback(&caller, &info);
            let idx = self.internal_record_feedback(
                agent_id,
                caller,
                value.0,
                value_decimals,
                tag1,
                tag2,
                endpoint,
                feedback_uri,
                parsed_hash,
            );
            return PromiseOrValue::Value(idx);
        }

        let promise = ext_factory::ext(self.factory.clone())
            .with_static_gas(GAS_FOR_GET_PERMISSIONS)
            .get_agent_permissions(agent_id, caller.clone())
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_ON_GIVE_FEEDBACK)
                    .on_give_feedback(
                        agent_id,
                        caller,
                        value,
                        value_decimals,
                        tag1,
                        tag2,
                        endpoint,
                        feedback_uri,
                        parsed_hash,
                    ),
            );
        PromiseOrValue::Promise(promise)
    }

    #[private]
    #[allow(clippy::too_many_arguments)]
    pub fn on_give_feedback(
        &mut self,
        agent_id: u64,
        caller: AccountId,
        value: I128,
        value_decimals: u8,
        tag1: String,
        tag2: String,
        endpoint: String,
        feedback_uri: String,
        feedback_hash: [u8; 32],
    ) -> u64 {
        let result_bytes = env::promise_result_checked(0, 4096)
            .unwrap_or_else(|_| Man0Error::CrossContractCallFailed.panic());
        let info: Option<AgentPermissionInfo> = serde_json::from_slice(&result_bytes)
            .unwrap_or_else(|_| Man0Error::CrossContractCallFailed.panic());
        let info = info.unwrap_or_else(|| Man0Error::AgentNotFound.panic());

        Self::assert_not_self_feedback(&caller, &info);
        self.cached_agents.insert(agent_id, info);

        self.internal_record_feedback(
            agent_id,
            caller,
            value.0,
            value_decimals,
            tag1,
            tag2,
            endpoint,
            feedback_uri,
            feedback_hash,
        )
    }
}
