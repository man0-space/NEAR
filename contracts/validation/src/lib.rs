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

    // =========================================================================
    // View Methods
    // =========================================================================

    pub fn get_validation_request(&self, request_id: u64) -> Option<ValidationRequestView> {
        self.requests.get(&request_id).map(|r| r.to_view())
    }

    pub fn get_validation_response(&self, response_id: u64) -> Option<ValidationResponseView> {
        self.responses.get(&response_id).map(|r| r.to_view())
    }

    pub fn get_agent_validation_requests(&self, agent_id: u64) -> Vec<u64> {
        self.agent_requests
            .get(&agent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_validation_responses(&self, request_id: u64) -> Vec<u64> {
        self.request_responses
            .get(&request_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn total_requests(&self) -> u64 {
        self.next_request_id.saturating_sub(1)
    }

    pub fn total_responses(&self) -> u64 {
        self.next_response_id.saturating_sub(1)
    }

    pub fn set_factory(&mut self, new_factory: AccountId) {
        man0_require!(
            env::predecessor_account_id() == self.admin,
            Man0Error::Unauthorized
        );
        self.factory = new_factory;
    }

    pub fn set_verify_via_xcc(&mut self, enabled: bool) {
        man0_require!(
            env::predecessor_account_id() == self.admin,
            Man0Error::Unauthorized
        );
        self.verify_via_xcc = enabled;
    }

    pub fn get_factory(&self) -> AccountId {
        self.factory.clone()
    }

    pub fn get_admin(&self) -> AccountId {
        self.admin.clone()
    }

    fn internal_create_request(
        &mut self,
        agent_id: u64,
        requester: AccountId,
        validator_type: String,
        task_hash: [u8; 32],
        request_uri: String,
    ) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());

        let record = ValidationRequest {
            request_id,
            agent_id,
            requester: requester.clone(),
            validator_type: validator_type.clone(),
            task_hash,
            request_uri: request_uri.clone(),
            status: ValidationStatus::Pending,
            created_at: env::block_timestamp(),
            response_count: 0,
        };
        self.requests.insert(request_id, record);

        let mut list = self
            .agent_requests
            .get(&agent_id)
            .cloned()
            .unwrap_or_default();
        list.push(request_id);
        self.agent_requests.insert(agent_id, list);

        Man0Event::ValidationRequested {
            request_id,
            agent_id,
            requester,
            validator_type,
            request_uri,
        }
        .emit();

        request_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use man0::hash::bytes32_to_hex;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    fn acc(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn setup_ctx(predecessor: &AccountId, timestamp_ns: u64) {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acc("validation.testnet"))
            .predecessor_account_id(predecessor.clone())
            .signer_account_id(predecessor.clone())
            .block_timestamp(timestamp_ns);
        testing_env!(builder.build());
    }

    fn setup_contract() -> ValidationContract {
        let admin = acc("admin.testnet");
        setup_ctx(&admin, 1_000);
        let mut contract = ValidationContract::new(admin, acc("factory.testnet"));
        contract.set_verify_via_xcc(false);
        contract
    }

    #[test]
    fn test_validation_lifecycle() {
        let mut contract = setup_contract();
        let alice = acc("alice.testnet");
        let validator = acc("tee-validator.testnet");

        setup_ctx(&alice, 2_000);
        let PromiseOrValue::Value(req_id) = contract.request_validation(
            1,
            "TEE".to_string(),
            bytes32_to_hex(&[0x33; 32]),
            "ipfs://req1".to_string(),
        ) else {
            panic!("Expected value");
        };
        assert_eq!(req_id, 1);
        assert_eq!(contract.total_requests(), 1);
        assert_eq!(contract.get_agent_validation_requests(1), vec![1]);

        setup_ctx(&validator, 3_000);
        let resp_id = contract.respond_validation(
            req_id,
            true,
            bytes32_to_hex(&[0x44; 32]),
            "ipfs://ev1".to_string(),
        );
        assert_eq!(resp_id, 1);
        assert_eq!(contract.total_responses(), 1);
        assert_eq!(contract.get_validation_responses(req_id), vec![1]);

        let req_view = contract.get_validation_request(req_id).unwrap();
        assert_eq!(req_view.status, ValidationStatus::Completed);
        assert_eq!(req_view.response_count, 1);

        setup_ctx(&alice, 4_000);
        contract.close_validation(req_id);
        assert_eq!(
            contract.get_validation_request(req_id).unwrap().status,
            ValidationStatus::Closed
        );
    }

    #[test]
    #[should_panic(expected = "ERR_NOT_REQUESTER")]
    fn test_non_requester_cannot_close() {
        let mut contract = setup_contract();
        let alice = acc("alice.testnet");
        let bob = acc("bob.testnet");
        setup_ctx(&alice, 2_000);
        let PromiseOrValue::Value(req_id) = contract.request_validation(
            1,
            "ZK".to_string(),
            String::new(),
            "ipfs://r".to_string(),
        ) else {
            panic!("Expected value");
        };
        setup_ctx(&bob, 3_000);
        contract.close_validation(req_id);
    }

    #[test]
    #[should_panic(expected = "ERR_REQUEST_ALREADY_CLOSED")]
    fn test_cannot_respond_to_closed_request() {
        let mut contract = setup_contract();
        let alice = acc("alice.testnet");
        let validator = acc("validator.testnet");
        setup_ctx(&alice, 2_000);
        let PromiseOrValue::Value(req_id) = contract.request_validation(
            1,
            "STAKE".to_string(),
            String::new(),
            "ipfs://r".to_string(),
        ) else {
            panic!("Expected value");
        };
        contract.close_validation(req_id);

        setup_ctx(&validator, 3_000);
        contract.respond_validation(req_id, false, String::new(), "ipfs://ev".to_string());
    }

    #[test]
    #[should_panic(expected = "ERR_REQUEST_ALREADY_CLOSED")]
    fn test_cannot_close_request_twice() {
        let mut contract = setup_contract();
        let alice = acc("alice.testnet");
        setup_ctx(&alice, 2_000);
        let PromiseOrValue::Value(req_id) = contract.request_validation(
            1,
            "TEE".to_string(),
            String::new(),
            "ipfs://r".to_string(),
        ) else {
            panic!("Expected value");
        };
        contract.close_validation(req_id);
        contract.close_validation(req_id);
    }

    #[test]
    fn test_multiple_validator_responses_per_request() {
        let mut contract = setup_contract();
        let alice = acc("alice.testnet");
        let v1 = acc("v1.testnet");
        let v2 = acc("v2.testnet");

        setup_ctx(&alice, 2_000);
        let PromiseOrValue::Value(req_id) = contract.request_validation(
            1,
            "ZKML".to_string(),
            String::new(),
            "ipfs://req".to_string(),
        ) else {
            panic!("Expected value");
        };

        setup_ctx(&v1, 3_000);
        let r1 = contract.respond_validation(req_id, true, String::new(), "ipfs://ev1".to_string());
        setup_ctx(&v2, 4_000);
        let r2 =
            contract.respond_validation(req_id, false, String::new(), "ipfs://ev2".to_string());

        assert_eq!(contract.get_validation_responses(req_id), vec![r1, r2]);
        assert_eq!(
            contract.get_validation_request(req_id).unwrap().response_count,
            2
        );
        assert!(contract.get_validation_response(r1).unwrap().valid);
        assert!(!contract.get_validation_response(r2).unwrap().valid);
    }

    #[test]
    fn test_xcc_on_request_validation_callback_success() {
        use near_sdk::json_types::{U128, U64};
        use near_sdk::PromiseResult;

        let admin = acc("admin.testnet");
        let factory = acc("factory.testnet");
        let alice = acc("alice.testnet");

        setup_ctx(&admin, 1_000);
        let mut contract = ValidationContract::new(admin, factory);

        let agent_view = Some(AgentView {
            agent_id: 1,
            dna_hash: bytes32_to_hex(&[0x11; 32]),
            owner: acc("owner.testnet"),
            creator: acc("owner.testnet"),
            licenses: 10,
            licenses_minted: 0,
            license_price: U128(100),
            creator_fee: 1,
            cloneable: false,
            is_clone: false,
            parent_agent_id: 0,
            agent_wallet: "owner.testnet".to_string(),
            uri: "ipfs://a".to_string(),
            created_at: U64(1_000),
        });
        let payload = serde_json::to_vec(&agent_view).unwrap();

        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acc("validation.testnet"))
            .predecessor_account_id(acc("validation.testnet"))
            .signer_account_id(alice.clone())
            .block_timestamp(2_000);

        testing_env!(
            builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![PromiseResult::Successful(payload)],
        );

        let req_id = contract.on_request_validation(
            1,
            alice,
            "TEE".to_string(),
            [0x77; 32],
            "ipfs://req-xcc".to_string(),
        );
        assert_eq!(req_id, 1);
        assert_eq!(contract.total_requests(), 1);
    }

    #[test]
    #[should_panic(expected = "ERR_AGENT_NOT_FOUND")]
    fn test_xcc_on_request_validation_missing_agent_panics() {
        use near_sdk::PromiseResult;
        let admin = acc("admin.testnet");
        let factory = acc("factory.testnet");
        let alice = acc("alice.testnet");

        setup_ctx(&admin, 1_000);
        let mut contract = ValidationContract::new(admin, factory);

        let payload = serde_json::to_vec(&None::<AgentView>).unwrap();
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acc("validation.testnet"))
            .predecessor_account_id(acc("validation.testnet"))
            .signer_account_id(alice.clone());

        testing_env!(
            builder.build(),
            near_sdk::test_vm_config(),
            near_sdk::RuntimeFeesConfig::test(),
            Default::default(),
            vec![PromiseResult::Successful(payload)],
        );

        contract.on_request_validation(999, alice, "TEE".to_string(), [0u8; 32], String::new());
    }
}
