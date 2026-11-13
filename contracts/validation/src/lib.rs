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
}
