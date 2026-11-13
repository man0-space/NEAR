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

    pub fn revoke_feedback(&mut self, agent_id: u64, feedback_index: u64) {
        let caller = env::predecessor_account_id();
        let last = self
            .last_index
            .get(&AgentClientPair {
                agent_id,
                client: caller.clone(),
            })
            .copied()
            .unwrap_or(0);
        man0_require!(
            feedback_index > 0 && feedback_index <= last,
            Man0Error::InvalidFeedbackIndex
        );

        let key = FeedbackKey {
            agent_id,
            client: caller.clone(),
            feedback_index,
        };
        let entry = self
            .feedback
            .get_mut(&key)
            .unwrap_or_else(|| Man0Error::InvalidFeedbackIndex.panic());
        man0_require!(!entry.is_revoked, Man0Error::FeedbackAlreadyRevoked);
        entry.is_revoked = true;

        Man0Event::FeedbackRevoked {
            agent_id,
            client: caller,
            feedback_index,
        }
        .emit();
    }

    pub fn append_response(
        &mut self,
        agent_id: u64,
        client_address: AccountId,
        feedback_index: u64,
        response_uri: String,
        response_hash: String,
    ) {
        man0_require!(agent_id > 0, Man0Error::AgentNotFound);
        validate_uri(&response_uri, false).unwrap_or_else(|e| e.panic());
        if !response_hash.is_empty() {
            let _ = hex_to_bytes32(&response_hash).unwrap_or_else(|e| e.panic());
        }

        let last = self
            .last_index
            .get(&AgentClientPair {
                agent_id,
                client: client_address.clone(),
            })
            .copied()
            .unwrap_or(0);
        man0_require!(
            feedback_index > 0 && feedback_index <= last,
            Man0Error::InvalidFeedbackIndex
        );

        let responder = env::predecessor_account_id();
        let fb_key = FeedbackKey {
            agent_id,
            client: client_address.clone(),
            feedback_index,
        };
        let mut resp_list = self
            .responders
            .get(&fb_key)
            .cloned()
            .unwrap_or_default();
        if !resp_list.contains(&responder) {
            resp_list.push(responder.clone());
            self.responders.insert(fb_key, resp_list);
        }

        let cnt_key = ResponseCountKey {
            agent_id,
            client: client_address.clone(),
            feedback_index,
            responder: responder.clone(),
        };
        let prev = self.response_counts.get(&cnt_key).copied().unwrap_or(0);
        self.response_counts
            .insert(cnt_key, prev.saturating_add(1));

        Man0Event::ResponseAppended {
            agent_id,
            client: client_address,
            feedback_index,
            responder,
            response_uri,
        }
        .emit();
    }

    // =========================================================================
    // View Methods
    // =========================================================================

    pub fn get_last_index(&self, agent_id: u64, client_address: AccountId) -> u64 {
        self.last_index
            .get(&AgentClientPair {
                agent_id,
                client: client_address,
            })
            .copied()
            .unwrap_or(0)
    }

    pub fn read_feedback(
        &self,
        agent_id: u64,
        client_address: AccountId,
        feedback_index: u64,
    ) -> Option<FeedbackView> {
        self.feedback
            .get(&FeedbackKey {
                agent_id,
                client: client_address,
                feedback_index,
            })
            .map(|r| r.to_view())
    }

    pub fn read_all_feedback(
        &self,
        agent_id: u64,
        client_addresses: Option<Vec<AccountId>>,
        tag1: Option<String>,
        tag2: Option<String>,
        include_revoked: Option<bool>,
    ) -> Vec<FeedbackView> {
        let clients = client_addresses.unwrap_or_else(|| {
            self.clients
                .get(&agent_id)
                .cloned()
                .unwrap_or_default()
        });
        let filter_t1 = tag1.unwrap_or_default();
        let filter_t2 = tag2.unwrap_or_default();
        let inc_revoked = include_revoked.unwrap_or(false);

        let mut out = Vec::new();
        for client in clients {
            let last = self.get_last_index(agent_id, client.clone());
            for idx in 1..=last {
                if let Some(entry) = self.feedback.get(&FeedbackKey {
                    agent_id,
                    client: client.clone(),
                    feedback_index: idx,
                }) {
                    if !inc_revoked && entry.is_revoked {
                        continue;
                    }
                    if !filter_t1.is_empty() && entry.tag1 != filter_t1 {
                        continue;
                    }
                    if !filter_t2.is_empty() && entry.tag2 != filter_t2 {
                        continue;
                    }
                    out.push(entry.to_view());
                }
            }
        }
        out
    }

    pub fn get_summary(
        &self,
        agent_id: u64,
        client_addresses: Vec<AccountId>,
        tag1: Option<String>,
        tag2: Option<String>,
    ) -> SummaryResult {
        man0_require!(
            !client_addresses.is_empty(),
            Man0Error::ClientAddressesRequired
        );
        let filter_t1 = tag1.unwrap_or_default();
        let filter_t2 = tag2.unwrap_or_default();

        let mut entries = Vec::new();
        for client in client_addresses {
            let last = self.get_last_index(agent_id, client.clone());
            for idx in 1..=last {
                if let Some(entry) = self.feedback.get(&FeedbackKey {
                    agent_id,
                    client: client.clone(),
                    feedback_index: idx,
                }) {
                    if entry.is_revoked {
                        continue;
                    }
                    if !filter_t1.is_empty() && entry.tag1 != filter_t1 {
                        continue;
                    }
                    if !filter_t2.is_empty() && entry.tag2 != filter_t2 {
                        continue;
                    }
                    entries.push((entry.value, entry.value_decimals));
                }
            }
        }

        let (count, val, mode) =
            aggregate_summary(&entries).unwrap_or_else(|e| e.panic());
        SummaryResult {
            count,
            summary_value: I128(val),
            summary_value_decimals: mode,
        }
    }

    pub fn get_response_count(
        &self,
        agent_id: u64,
        client_address: Option<AccountId>,
        feedback_index: Option<u64>,
        responders: Option<Vec<AccountId>>,
    ) -> u64 {
        let client_list = match client_address {
            Some(c) => vec![c],
            None => self.clients.get(&agent_id).cloned().unwrap_or_default(),
        };
        let resp_filter = responders.unwrap_or_default();
        let idx_filter = feedback_index.unwrap_or(0);

        let mut total: u64 = 0;
        for client in client_list {
            let last = self.get_last_index(agent_id, client.clone());
            let range = if idx_filter > 0 {
                if idx_filter <= last {
                    idx_filter..=idx_filter
                } else {
                    1..=0
                }
            } else {
                1..=last
            };
            for idx in range {
                let fb_key = FeedbackKey {
                    agent_id,
                    client: client.clone(),
                    feedback_index: idx,
                };
                let candidates = if resp_filter.is_empty() {
                    self.responders.get(&fb_key).cloned().unwrap_or_default()
                } else {
                    resp_filter.clone()
                };
                for r in candidates {
                    total = total.saturating_add(
                        self.response_counts
                            .get(&ResponseCountKey {
                                agent_id,
                                client: client.clone(),
                                feedback_index: idx,
                                responder: r,
                            })
                            .copied()
                            .unwrap_or(0),
                    );
                }
            }
        }
        total
    }

    pub fn get_clients(&self, agent_id: u64) -> Vec<AccountId> {
        self.clients.get(&agent_id).cloned().unwrap_or_default()
    }

    pub fn get_responders(
        &self,
        agent_id: u64,
        client_address: AccountId,
        feedback_index: u64,
    ) -> Vec<AccountId> {
        self.responders
            .get(&FeedbackKey {
                agent_id,
                client: client_address,
                feedback_index,
            })
            .cloned()
            .unwrap_or_default()
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

    pub fn cache_agent_permissions(&mut self, agent_id: u64, info: AgentPermissionInfo) {
        let caller = env::predecessor_account_id();
        man0_require!(
            caller == self.admin || caller == self.factory,
            Man0Error::Unauthorized
        );
        self.cached_agents.insert(agent_id, info);
    }

    pub fn get_factory(&self) -> AccountId {
        self.factory.clone()
    }

    pub fn get_admin(&self) -> AccountId {
        self.admin.clone()
    }

    // =========================================================================
    // Internal Helpers
    // =========================================================================

    fn assert_not_self_feedback(caller: &AccountId, info: &AgentPermissionInfo) {
        man0_require!(info.exists, Man0Error::AgentNotFound);
        man0_require!(
            caller != &info.owner
                && caller.as_str() != info.agent_wallet.as_str()
                && !info.is_approved_operator,
            Man0Error::SelfFeedbackNotAllowed
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn internal_record_feedback(
        &mut self,
        agent_id: u64,
        client: AccountId,
        value: i128,
        value_decimals: u8,
        tag1: String,
        tag2: String,
        endpoint: String,
        feedback_uri: String,
        feedback_hash: [u8; 32],
    ) -> u64 {
        let pair = AgentClientPair {
            agent_id,
            client: client.clone(),
        };
        let prev = self.last_index.get(&pair).copied().unwrap_or(0);
        let next_idx = prev
            .checked_add(1)
            .unwrap_or_else(|| Man0Error::ArithmeticOverflow.panic());
        self.last_index.insert(pair, next_idx);

        if prev == 0 {
            let mut client_list = self
                .clients
                .get(&agent_id)
                .cloned()
                .unwrap_or_default();
            if !client_list.contains(&client) {
                client_list.push(client.clone());
                self.clients.insert(agent_id, client_list);
            }
        }

        let record = FeedbackRecord {
            agent_id,
            client: client.clone(),
            feedback_index: next_idx,
            value,
            value_decimals,
            tag1: tag1.clone(),
            tag2: tag2.clone(),
            endpoint: endpoint.clone(),
            feedback_uri: feedback_uri.clone(),
            feedback_hash,
            is_revoked: false,
            created_at: env::block_timestamp(),
        };
        self.feedback.insert(
            FeedbackKey {
                agent_id,
                client: client.clone(),
                feedback_index: next_idx,
            },
            record,
        );

        Man0Event::NewFeedback {
            agent_id,
            client,
            feedback_index: next_idx,
            value: I128(value),
            value_decimals,
            tag1,
            tag2,
            endpoint,
            feedback_uri,
        }
        .emit();

        next_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use man0::hash::bytes32_to_hex;
    use near_sdk::json_types::U128;
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    fn acc(name: &str) -> AccountId {
        name.parse().unwrap()
    }

    fn setup_ctx(predecessor: &AccountId, timestamp_ns: u64) {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(acc("reputation.testnet"))
            .predecessor_account_id(predecessor.clone())
            .signer_account_id(predecessor.clone())
            .block_timestamp(timestamp_ns);
        testing_env!(builder.build());
    }

    fn setup_contract() -> ReputationContract {
        let admin = acc("admin.testnet");
        let factory = acc("factory.testnet");
        setup_ctx(&admin, 1_000);
        let mut contract = ReputationContract::new(admin.clone(), factory);
        contract.set_verify_via_xcc(false);
        contract.cache_agent_permissions(
            1,
            AgentPermissionInfo {
                exists: true,
                owner: acc("owner.testnet"),
                creator: acc("owner.testnet"),
                agent_wallet: "agent-wallet.testnet".to_string(),
                is_approved_operator: false,
                license_price: U128(100),
                is_warped: false,
                warp_creator: None,
            },
        );
        contract
    }

    #[test]
    fn test_give_revoke_append_and_summary_happy_path() {
        let mut contract = setup_contract();
        let bob = acc("bob.testnet");
        let charlie = acc("charlie.testnet");
        let responder = acc("responder.testnet");

        setup_ctx(&bob, 2_000);
        let PromiseOrValue::Value(idx1) = contract.give_feedback(
            1,
            I128(950),
            2, // 9.50
            "quality".to_string(),
            "speed".to_string(),
            "/v1/chat".to_string(),
            "ipfs://fb1".to_string(),
            bytes32_to_hex(&[0x11; 32]),
        ) else {
            panic!("Expected value");
        };
        assert_eq!(idx1, 1);

        let PromiseOrValue::Value(idx2) = contract.give_feedback(
            1,
            I128(850),
            2, // 8.50
            "quality".to_string(),
            "cost".to_string(),
            "/v1/chat".to_string(),
            "ipfs://fb2".to_string(),
            String::new(),
        ) else {
            panic!("Expected value");
        };
        assert_eq!(idx2, 2);

        setup_ctx(&charlie, 3_000);
        let PromiseOrValue::Value(c_idx1) = contract.give_feedback(
            1,
            I128(6_000_000),
            6, // 6.000000
            "quality".to_string(),
            "speed".to_string(),
            "/v1/chat".to_string(),
            "ipfs://fb3".to_string(),
            String::new(),
        ) else {
            panic!("Expected value");
        };
        assert_eq!(c_idx1, 1);

        // Summary across bob & charlie: (9.50 + 8.50 + 6.00) / 3 = 8.00 (mode decimals = 2 -> 800)
        let summary = contract.get_summary(1, vec![bob.clone(), charlie.clone()], None, None);
        assert_eq!(summary.count, 3);
        assert_eq!(summary.summary_value_decimals, 2);
        assert_eq!(summary.summary_value, I128(800));

        // Filter by tag2 = "speed": (9.50 + 6.00) / 2 = 7.75
        let speed_summary = contract.get_summary(
            1,
            vec![bob.clone(), charlie.clone()],
            Some("quality".to_string()),
            Some("speed".to_string()),
        );
        assert_eq!(speed_summary.count, 2);
        assert_eq!(speed_summary.summary_value, I128(775));

        // Revoke bob's second feedback (8.50)
        setup_ctx(&bob, 4_000);
        contract.revoke_feedback(1, 2);
        let after_revoke = contract.get_summary(1, vec![bob.clone(), charlie.clone()], None, None);
        assert_eq!(after_revoke.count, 2);
        assert_eq!(after_revoke.summary_value, I128(775));

        // Append response
        setup_ctx(&responder, 5_000);
        contract.append_response(
            1,
            bob.clone(),
            1,
            "ipfs://resp1".to_string(),
            bytes32_to_hex(&[0x22; 32]),
        );
        contract.append_response(
            1,
            bob.clone(),
            1,
            "ipfs://resp2".to_string(),
            String::new(),
        );
        assert_eq!(contract.get_response_count(1, None, None, None), 2);
        assert_eq!(contract.get_responders(1, bob, 1), vec![responder]);
    }

    #[test]
    #[should_panic(expected = "ERR_SELF_FEEDBACK_NOT_ALLOWED")]
    fn test_self_feedback_by_owner_rejected() {
        let mut contract = setup_contract();
        let owner = acc("owner.testnet");
        setup_ctx(&owner, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(100),
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
    }

    #[test]
    #[should_panic(expected = "ERR_SELF_FEEDBACK_NOT_ALLOWED")]
    fn test_self_feedback_by_agent_wallet_rejected() {
        let mut contract = setup_contract();
        let wallet = acc("agent-wallet.testnet");
        setup_ctx(&wallet, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(100),
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
    }

    #[test]
    #[should_panic(expected = "ERR_FEEDBACK_ALREADY_REVOKED")]
    fn test_double_revoke_feedback_rejected() {
        let mut contract = setup_contract();
        let bob = acc("bob.testnet");
        setup_ctx(&bob, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(500),
            2,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
        contract.revoke_feedback(1, 1);
        contract.revoke_feedback(1, 1);
    }

    #[test]
    #[should_panic(expected = "ERR_CLIENT_ADDRESSES_REQUIRED")]
    fn test_get_summary_requires_non_empty_clients() {
        let contract = setup_contract();
        let _ = contract.get_summary(1, Vec::new(), None, None);
    }

    #[test]
    #[should_panic(expected = "ERR_SELF_FEEDBACK_NOT_ALLOWED")]
    fn test_self_feedback_by_approved_operator_rejected() {
        let mut contract = setup_contract();
        let admin = acc("admin.testnet");
        let operator = acc("operator.testnet");
        setup_ctx(&admin, 1_500);
        contract.cache_agent_permissions(
            2,
            AgentPermissionInfo {
                exists: true,
                owner: acc("owner.testnet"),
                creator: acc("owner.testnet"),
                agent_wallet: "owner.testnet".to_string(),
                is_approved_operator: true,
                license_price: U128(0),
                is_warped: false,
                warp_creator: None,
            },
        );
        setup_ctx(&operator, 2_000);
        let _ = contract.give_feedback(
            2,
            I128(100),
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
    }

    #[test]
    #[should_panic(expected = "ERR_INVALID_DECIMALS")]
    fn test_give_feedback_decimals_above_18_rejected() {
        let mut contract = setup_contract();
        let bob = acc("bob.testnet");
        setup_ctx(&bob, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(100),
            19,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
    }

    #[test]
    #[should_panic(expected = "ERR_INVALID_FEEDBACK_INDEX")]
    fn test_revoke_feedback_wrong_author_or_index_rejected() {
        let mut contract = setup_contract();
        let bob = acc("bob.testnet");
        let attacker = acc("attacker.testnet");
        setup_ctx(&bob, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(100),
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
        setup_ctx(&attacker, 3_000);
        contract.revoke_feedback(1, 1);
    }

    #[test]
    #[should_panic(expected = "ERR_EMPTY_URI")]
    fn test_append_response_empty_uri_rejected() {
        let mut contract = setup_contract();
        let bob = acc("bob.testnet");
        setup_ctx(&bob, 2_000);
        let _ = contract.give_feedback(
            1,
            I128(100),
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
        contract.append_response(1, bob, 1, "   ".to_string(), String::new());
    }
}
