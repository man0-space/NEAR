use near_sdk::json_types::{I128, U128};
use near_sdk::{near, AccountId};

#[near(event_json(standard = "nep171"))]
pub enum Nep171Event {
    #[event_version("1.0.0")]
    NftMint {
        owner_id: AccountId,
        token_ids: Vec<String>,
        memo: Option<String>,
    },
    #[event_version("1.0.0")]
    NftTransfer {
        authorized_id: Option<AccountId>,
        old_owner_id: AccountId,
        new_owner_id: AccountId,
        token_ids: Vec<String>,
        memo: Option<String>,
    },
}

#[near(event_json(standard = "man0"))]
pub enum Man0Event {
    #[event_version("1.0.0")]
    AgentRegistered {
        agent_id: u64,
        owner: AccountId,
        creator: AccountId,
        dna_hash: String,
        licenses: u64,
        license_price: U128,
        cloneable: bool,
        uri: String,
    },
    #[event_version("1.0.0")]
    AgentCloned {
        original_agent_id: u64,
        cloned_agent_id: u64,
        cloner: AccountId,
        chain_id: u64,
        model: String,
        license_price: U128,
    },
    #[event_version("1.0.0")]
    AgentWarped {
        warped_agent_id: u64,
        original_hash: String,
        original_creator: Option<AccountId>,
        warper: AccountId,
        licenses: u64,
        license_price: U128,
    },
    #[event_version("1.0.0")]
    UnclaimedWarpRoyaltiesTransferred {
        warped_agent_id: u64,
        verified_creator: AccountId,
        amount: U128,
    },
    #[event_version("1.0.0")]
    LicenseConsumed {
        agent_id: u64,
        workflow_contract: AccountId,
        workflow_id: u64,
        price_paid: U128,
    },
    #[event_version("1.0.0")]
    LicenseRevoked {
        agent_id: u64,
        workflow_contract: AccountId,
        workflow_id: u64,
    },
    #[event_version("1.0.0")]
    PriceUpdated {
        agent_id: u64,
        old_price: U128,
        new_price: U128,
    },
    #[event_version("1.0.0")]
    AgentWalletUpdated {
        agent_id: u64,
        new_wallet: String,
    },
    #[event_version("1.0.0")]
    MetadataUpdated {
        agent_id: u64,
        key: String,
    },
    #[event_version("1.0.0")]
    NewFeedback {
        agent_id: u64,
        client: AccountId,
        feedback_index: u64,
        value: I128,
        value_decimals: u8,
        tag1: String,
        tag2: String,
        endpoint: String,
        feedback_uri: String,
    },
    #[event_version("1.0.0")]
    FeedbackRevoked {
        agent_id: u64,
        client: AccountId,
        feedback_index: u64,
    },
    #[event_version("1.0.0")]
    ResponseAppended {
        agent_id: u64,
        client: AccountId,
        feedback_index: u64,
        responder: AccountId,
        response_uri: String,
    },
    #[event_version("1.0.0")]
    ValidationRequested {
        request_id: u64,
        agent_id: u64,
        requester: AccountId,
        validator_type: String,
        request_uri: String,
    },
    #[event_version("1.0.0")]
    ValidationResponded {
        response_id: u64,
        request_id: u64,
        validator: AccountId,
        valid: bool,
        evidence_uri: String,
    },
    #[event_version("1.0.0")]
    ValidationClosed {
        request_id: u64,
        requester: AccountId,
    },
    #[event_version("1.0.0")]
    WorkflowMinted {
        workflow_id: u64,
        creator: AccountId,
        title: String,
        total_price: U128,
        units: u64,
    },
    #[event_version("1.0.0")]
    AgentAddedToWorkflow {
        workflow_id: u64,
        agent_id: u64,
        license_price: U128,
    },
    #[event_version("1.0.0")]
    AgentRemovedFromWorkflow {
        workflow_id: u64,
        agent_id: u64,
        license_price: U128,
    },
    #[event_version("1.0.0")]
    ChildProposed {
        parent_token_id: u64,
        child_contract: AccountId,
        child_token_id: u64,
    },
    #[event_version("1.0.0")]
    ChildAccepted {
        parent_token_id: u64,
        child_contract: AccountId,
        child_token_id: u64,
    },
    #[event_version("1.0.0")]
    ChildRejected {
        parent_token_id: u64,
        child_contract: AccountId,
        child_token_id: u64,
    },
    #[event_version("1.0.0")]
    ChildUnnested {
        parent_token_id: u64,
        child_contract: AccountId,
        child_token_id: u64,
    },
    #[event_version("1.0.0")]
    RfaCreated {
        rfa_id: u64,
        workflow_id: u64,
        publisher: AccountId,
        offer_amount: U128,
    },
    #[event_version("1.0.0")]
    AgentSubmittedToRfa {
        rfa_id: u64,
        agent_id: u64,
        creator: AccountId,
    },
    #[event_version("1.0.0")]
    RfaFulfilled {
        rfa_id: u64,
        workflow_id: u64,
        agent_id: u64,
        agent_creator: AccountId,
        offer_amount: U128,
    },
    #[event_version("1.0.0")]
    RfaCancelled {
        rfa_id: u64,
        workflow_id: u64,
        publisher: AccountId,
        refunded_amount: U128,
    },
    #[event_version("1.0.0")]
    LeaseCreated {
        lease_id: u64,
        workflow_id: u64,
        leaser: AccountId,
        start_time: u64,
        end_time: u64,
        creator_percent: u8,
    },
    #[event_version("1.0.0")]
    LeaseTerminated {
        lease_id: u64,
        workflow_id: u64,
        terminated_by: AccountId,
    },
    #[event_version("1.0.0")]
    LeaseExpired {
        lease_id: u64,
        workflow_id: u64,
    },
    #[event_version("1.0.0")]
    LeaseFeesDistributed {
        lease_id: u64,
        workflow_id: u64,
        creator_amount: U128,
        leaser_amount: U128,
    },
}
