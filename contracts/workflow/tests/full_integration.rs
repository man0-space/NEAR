use factory::FactoryContract;
use man0::hash::bytes32_to_hex;
use man0::state::{CloneParams, MintWorkflowParams, RfaStatus, ValidationStatus};
use near_sdk::json_types::{I128, U128};
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::{testing_env, AccountId, NearToken, PromiseResult};
use reputation::ReputationContract;
use validation::ValidationContract;
use workflow::WorkflowContract;

fn acc(name: &str) -> AccountId {
    name.parse().unwrap()
}

fn set_ctx(current: &AccountId, predecessor: &AccountId, deposit_yocto: u128, ts: u64) {
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(current.clone())
        .predecessor_account_id(predecessor.clone())
        .signer_account_id(predecessor.clone())
        .account_balance(NearToken::from_near(100))
        .attached_deposit(NearToken::from_yoctonear(deposit_yocto))
        .block_timestamp(ts);
    testing_env!(builder.build());
}

fn set_callback_ctx(
    current: &AccountId,
    signer: &AccountId,
    ts: u64,
    promise_results: Vec<PromiseResult>,
) {
    let mut builder = VMContextBuilder::new();
    builder
        .current_account_id(current.clone())
        .predecessor_account_id(current.clone())
        .signer_account_id(signer.clone())
        .account_balance(NearToken::from_near(100))
        .block_timestamp(ts);
    testing_env!(
        builder.build(),
        near_sdk::test_vm_config(),
        near_sdk::RuntimeFeesConfig::test(),
        Default::default(),
        promise_results,
    );
}

#[test]
fn test_end_to_end_man0_protocol_across_all_four_programs() {
    let admin = acc("admin.testnet");
    let treasury = acc("treasury.testnet");
    let alice = acc("alice.testnet");
    let bob = acc("bob.testnet");
    let charlie = acc("charlie.testnet");
    let validator_node = acc("validator.testnet");

    let factory_id = acc("factory.testnet");
    let reputation_id = acc("reputation.testnet");
    let validation_id = acc("validation.testnet");
    let workflow_id = acc("workflow.testnet");

    // 1. Initialize all 4 contracts
    set_ctx(&factory_id, &admin, 0, 1_000);
    let mut factory = FactoryContract::new(admin.clone(), treasury.clone(), None);
    factory.set_authorized_consumer(workflow_id.clone(), true);

    set_ctx(&reputation_id, &admin, 0, 1_000);
    let mut reputation = ReputationContract::new(admin.clone(), factory_id.clone());

    set_ctx(&validation_id, &admin, 0, 1_000);
    let mut validation = ValidationContract::new(admin.clone(), factory_id.clone());

    set_ctx(&workflow_id, &admin, 0, 1_000);
    let mut workflow =
        WorkflowContract::new(admin.clone(), treasury.clone(), factory_id.clone(), None);

    // 2. Alice mints Agent #1 (cloneable) and Agent #2 on Factory
    set_ctx(&factory_id, &alice, 0, 2_000);
    let agent1 = factory.mint_agent(
        bytes32_to_hex(&[0x01; 32]),
        100,
        U128(500_000),
        Some(5),
        true,
        "ipfs://agent-1".to_string(),
        None,
    );
    let agent2 = factory.mint_agent(
        bytes32_to_hex(&[0x02; 32]),
        50,
        U128(300_000),
        Some(3),
        false,
        "ipfs://agent-2".to_string(),
        None,
    );
    assert_eq!(agent1, 1);
    assert_eq!(agent2, 2);

    // 3. Bob clones Agent #1 -> Agent #3
    set_ctx(&factory_id, &bob, 0, 3_000);
    let cloned_agent = factory.clone_agent(
        agent1,
        CloneParams {
            chain_id: 397,
            license_price: U128(400_000),
            model: "deepseek-r2".to_string(),
            licenses: 25,
        },
        "ipfs://agent-clone".to_string(),
    );
    assert_eq!(cloned_agent, 3);

    // 4. Charlie warps an external agent -> Agent #4
    set_ctx(&factory_id, &charlie, 0, 4_000);
    let warped_agent = factory.warp_agent(
        bytes32_to_hex(&[0xfe; 32]),
        None,
        100,
        U128(200_000),
        "ipfs://agent-warped".to_string(),
    );
    assert_eq!(warped_agent, 4);
    assert_eq!(factory.total_agents(), 4);

    // 5. Reputation: Bob gives feedback on Agent #1 via Factory XCC -> Reputation callback
    set_ctx(&factory_id, &reputation_id, 0, 5_000);
    let perm_for_bob = factory.get_agent_permissions(agent1, bob.clone());
    let perm_bytes = serde_json::to_vec(&perm_for_bob).unwrap();

    set_ctx(&reputation_id, &bob, 0, 5_000);
    let _ = reputation.give_feedback(
        agent1,
        I128(925),
        2,
        "accuracy".to_string(),
        "reasoning".to_string(),
        "/v1/infer".to_string(),
        "ipfs://fb-bob".to_string(),
        bytes32_to_hex(&[0xaa; 32]),
    );

    set_callback_ctx(
        &reputation_id,
        &bob,
        5_001,
        vec![PromiseResult::Successful(perm_bytes)],
    );
    let fb_idx = reputation.on_give_feedback(
        agent1,
        bob.clone(),
        I128(925),
        2,
        "accuracy".to_string(),
        "reasoning".to_string(),
        "/v1/infer".to_string(),
        "ipfs://fb-bob".to_string(),
        [0xaa; 32],
    );
    assert_eq!(fb_idx, 1);

    let summary = reputation.get_summary(agent1, vec![bob.clone()], None, None);
    assert_eq!(summary.count, 1);
    assert_eq!(summary.summary_value, I128(925));
    assert_eq!(summary.summary_value_decimals, 2);

    // 6. Validation: Bob requests validation on Agent #1 via Factory XCC -> Validation callback
    set_ctx(&factory_id, &validation_id, 0, 6_000);
    let agent1_view = factory.get_agent(agent1);
    let agent1_bytes = serde_json::to_vec(&agent1_view).unwrap();

    set_callback_ctx(
        &validation_id,
        &bob,
        6_001,
        vec![PromiseResult::Successful(agent1_bytes)],
    );
    let req_id = validation.on_request_validation(
        agent1,
        bob.clone(),
        "TEE".to_string(),
        [0xbb; 32],
        "ipfs://val-req".to_string(),
    );
    assert_eq!(req_id, 1);

    set_ctx(&validation_id, &validator_node, 0, 6_500);
    let resp_id = validation.respond_validation(
        req_id,
        true,
        bytes32_to_hex(&[0xcc; 32]),
        "ipfs://val-ev".to_string(),
    );
    assert_eq!(resp_id, 1);
    assert_eq!(
        validation.get_validation_request(req_id).unwrap().status,
        ValidationStatus::Completed
    );

    set_ctx(&validation_id, &bob, 0, 6_600);
    validation.close_validation(req_id);
    assert_eq!(
        validation.get_validation_request(req_id).unwrap().status,
        ValidationStatus::Closed
    );

    // 7. Workflow: Alice mints a Workflow combining agent2 (300_000), cloned_agent (400_000), and warped_agent (200_000)
    // Consume licenses on Factory and feed XCC results into Workflow
    for &aid in &[agent2, cloned_agent, warped_agent] {
        set_ctx(&factory_id, &workflow_id, 0, 7_000);
        let perm = factory.get_agent_permissions(aid, alice.clone()).unwrap();
        set_ctx(&workflow_id, &admin, 0, 7_000);
        workflow.cache_agent_info(aid, perm);
    }

    set_ctx(&workflow_id, &alice, 0, 7_500);
    let wf_id = workflow.mint_workflow(
        MintWorkflowParams {
            title: "man0 Multi-Agent Pipeline".to_string(),
            description: "End-to-end orchestration".to_string(),
            banner: "ipfs://banner".to_string(),
            workflow_card_uri: "ipfs://wf-card".to_string(),
            units: 25,
            lease_enabled: true,
            lease_duration: 60,
            lease_percent: 10,
            has_coordinator: true,
            coordinator_model: "deepseek-r2".to_string(),
        },
        vec![agent2, cloned_agent, warped_agent],
    );
    assert_eq!(wf_id, 1);

    // Execute license consumption on Factory for each nested agent
    set_ctx(&factory_id, &workflow_id, 0, 7_600);
    for &aid in &[agent2, cloned_agent, warped_agent] {
        let info = factory.consume_license(aid, workflow_id.clone(), wf_id);
        let info_bytes = serde_json::to_vec(&info).unwrap();
        set_callback_ctx(
            &workflow_id,
            &alice,
            7_601,
            vec![PromiseResult::Successful(info_bytes)],
        );
        workflow.on_consume_license(wf_id, aid, info.license_price);
        assert!(factory.has_active_license(aid, workflow_id.clone(), wf_id));
    }

    let wf_data = workflow.get_workflow(wf_id).unwrap();
    assert_eq!(wf_data.total_price, U128(900_000));
    assert_eq!(workflow.get_agent_count(wf_id), 3);
    assert!(workflow.is_complete(wf_id));

    // 8. RFA on Workflow: Alice opens RFA, Bob submits cloned_agent, Alice accepts
    set_ctx(&workflow_id, &alice, 0, 8_000);
    let rfa_id = workflow.create_rfa(
        wf_id,
        "Specialist Agent Needed".to_string(),
        "Add specialist reasoning node".to_string(),
        vec!["reasoning".to_string()],
        U128(250_000),
    );
    assert!(!workflow.is_complete(wf_id));

    set_ctx(&workflow_id, &bob, 0, 8_100);
    workflow.submit_agent(rfa_id, cloned_agent);

    set_ctx(&workflow_id, &alice, 0, 8_200);
    workflow.accept_agent(rfa_id, cloned_agent);
    assert_eq!(
        workflow.get_rfa(rfa_id).unwrap().status,
        RfaStatus::Fulfilled
    );
    assert!(workflow.is_complete(wf_id));

    // 9. Lease on Workflow: Bob leases Workflow #1 for 30 days and distributes fees
    set_ctx(&workflow_id, &bob, 0, 9_000);
    let lease_id = workflow.create_lease(wf_id, 30);
    assert!(workflow.is_leased(wf_id));

    set_ctx(&workflow_id, &bob, 0, 9_500);
    workflow.distribute_lease_fees(lease_id, U128(100_000));
    let (c_split, l_split) = workflow.calculate_fee_split(lease_id, U128(100_000));
    assert_eq!(c_split, U128(10_000));
    assert_eq!(l_split, U128(90_000));
}
