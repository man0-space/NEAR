use man0::error::Man0Error;
use man0::events::Man0Event;
use man0::ext::ext_factory;
use man0::hash::hex_to_bytes32;
use man0::man0_require;
use man0::metadata::validate_uri;
use man0::state::{
    AgentView, ValidationRequest, ValidationRequestView, ValidationResponse,
    ValidationResponseView, ValidationStatus,
};
use near_sdk::store::LookupMap;
use near_sdk::{
    env, near, AccountId, BorshStorageKey, Gas, PanicOnDefault, PromiseOrValue,
};

const GAS_FOR_GET_AGENT: Gas = Gas::from_tgas(10);
const GAS_FOR_ON_REQUEST_VALIDATION: Gas = Gas::from_tgas(20);

#[near(serializers = [borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Requests,
    Responses,
    AgentRequests,
    RequestResponses,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct ValidationContract {
    admin: AccountId,
    factory: AccountId,
    verify_via_xcc: bool,
    next_request_id: u64,
    next_response_id: u64,
    requests: LookupMap<u64, ValidationRequest>,
    responses: LookupMap<u64, ValidationResponse>,
    agent_requests: LookupMap<u64, Vec<u64>>,
    request_responses: LookupMap<u64, Vec<u64>>,
}

#[near]
impl ValidationContract {
    #[init]
    pub fn new(admin: AccountId, factory: AccountId) -> Self {
        Self {
            admin,
            factory,
            verify_via_xcc: true,
            next_request_id: 1,
            next_response_id: 1,
            requests: LookupMap::new(StorageKey::Requests),
            responses: LookupMap::new(StorageKey::Responses),
            agent_requests: LookupMap::new(StorageKey::AgentRequests),
            request_responses: LookupMap::new(StorageKey::RequestResponses),
        }
    }

    pub fn request_validation(
        &mut self,
        agent_id: u64,
        validator_type: String,
        task_hash: String,
        request_uri: String,
    ) -> PromiseOrValue<u64> {
        man0_require!(agent_id > 0, Man0Error::AgentNotFound);
        validate_uri(&request_uri, true).unwrap_or_else(|e| e.panic());
        let parsed_hash = if task_hash.is_empty() {
            [0u8; 32]
        } else {
            hex_to_bytes32(&task_hash).unwrap_or_else(|e| e.panic())
        };

        let requester = env::predecessor_account_id();

        if !self.verify_via_xcc {
            let req_id = self.internal_create_request(
                agent_id,
                requester,
                validator_type,
                parsed_hash,
                request_uri,
            );
            return PromiseOrValue::Value(req_id);
        }

        let promise = ext_factory::ext(self.factory.clone())
            .with_static_gas(GAS_FOR_GET_AGENT)
            .get_agent(agent_id)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_FOR_ON_REQUEST_VALIDATION)
                    .on_request_validation(
                        agent_id,
                        requester,
                        validator_type,
                        parsed_hash,
                        request_uri,
                    ),
            );
        PromiseOrValue::Promise(promise)
    }

    #[private]
    pub fn on_request_validation(
        &mut self,
        agent_id: u64,
        requester: AccountId,
        validator_type: String,
        task_hash: [u8; 32],
        request_uri: String,
    ) -> u64 {
        let bytes = env::promise_result_checked(0, 4096)
            .unwrap_or_else(|_| Man0Error::CrossContractCallFailed.panic());
        let agent: Option<AgentView> = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Man0Error::CrossContractCallFailed.panic());
        man0_require!(agent.is_some(), Man0Error::AgentNotFound);

        self.internal_create_request(
            agent_id,
            requester,
            validator_type,
            task_hash,
            request_uri,
        )
    }
}
