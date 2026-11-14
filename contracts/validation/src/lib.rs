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

    pub fn respond_validation(
        &mut self,
        request_id: u64,
        valid: bool,
        evidence_hash: String,
        evidence_uri: String,
    ) -> u64 {
        validate_uri(&evidence_uri, true).unwrap_or_else(|e| e.panic());
        let parsed_hash = if evidence_hash.is_empty() {
            [0u8; 32]
        } else {
            hex_to_bytes32(&evidence_hash).unwrap_or_else(|e| e.panic())
        };

        let req = self
            .requests
            .get_mut(&request_id)
            .unwrap_or_else(|| Man0Error::ValidationRequestNotFound.panic());
        man0_require!(
            req.status != ValidationStatus::Closed,
            Man0Error::RequestAlreadyClosed
        );
        req.status = ValidationStatus::Completed;
        req.response_count = req.response_count.saturating_add(1);

        let response_id = self.next_response_id;
        self.next_response_id = self
            .next_response_id
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());

        let validator = env::predecessor_account_id();
        let response = ValidationResponse {
            response_id,
            request_id,
            validator: validator.clone(),
            valid,
            evidence_hash: parsed_hash,
            evidence_uri: evidence_uri.clone(),
            responded_at: env::block_timestamp(),
        };
        self.responses.insert(response_id, response);

        let mut list = self
            .request_responses
            .get(&request_id)
            .cloned()
            .unwrap_or_default();
        list.push(response_id);
        self.request_responses.insert(request_id, list);

        Man0Event::ValidationResponded {
            response_id,
            request_id,
            validator,
            valid,
            evidence_uri,
        }
        .emit();

        response_id
    }

    pub fn close_validation(&mut self, request_id: u64) {
        let req = self
            .requests
            .get_mut(&request_id)
            .unwrap_or_else(|| Man0Error::ValidationRequestNotFound.panic());
        let caller = env::predecessor_account_id();
        man0_require!(caller == req.requester, Man0Error::NotRequester);
        man0_require!(
            req.status != ValidationStatus::Closed,
            Man0Error::RequestAlreadyClosed
        );
        req.status = ValidationStatus::Closed;

        Man0Event::ValidationClosed {
            request_id,
            requester: caller,
        }
        .emit();
    }
}
