use crate::state::{AgentPermissionInfo, AgentView, MintWorkflowParams};
use near_sdk::json_types::U128;
use near_sdk::{ext_contract, near, AccountId};
use std::collections::HashMap;

#[ext_contract(ext_factory)]
pub trait ExtFactory {
    fn get_agent(&self, agent_id: u64) -> Option<AgentView>;
    fn get_agent_permissions(
        &self,
        agent_id: u64,
        candidate: AccountId,
    ) -> Option<AgentPermissionInfo>;
    fn consume_license(
        &mut self,
        agent_id: u64,
        workflow_contract: AccountId,
        workflow_id: u64,
    ) -> AgentPermissionInfo;
    fn revoke_license(&mut self, agent_id: u64, workflow_contract: AccountId, workflow_id: u64);
    fn record_unclaimed_warp_royalty(&mut self, warped_agent_id: u64, amount: U128);
}

#[ext_contract(ext_ft)]
pub trait ExtFungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[ext_contract(ext_nft_receiver)]
pub trait ExtNftReceiver {
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: String,
        msg: String,
    ) -> bool;
}

#[ext_contract(ext_nft_approval_receiver)]
pub trait ExtNftApprovalReceiver {
    fn nft_on_approve(
        &mut self,
        token_id: String,
        owner_id: AccountId,
        approval_id: u64,
        msg: String,
    );
}

#[ext_contract(ext_nft_resolver)]
pub trait ExtNftResolver {
    fn nft_resolve_transfer(
        &mut self,
        previous_owner_id: AccountId,
        receiver_id: AccountId,
        token_id: String,
        approved_account_ids: Option<HashMap<AccountId, u64>>,
        previous_agent_wallet: Option<String>,
    ) -> bool;
}

/// Structured NEP-141 `ft_transfer_call` payload accepted by `workflow::ft_on_transfer`.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum WorkflowFtAction {
    MintWorkflow {
        params: MintWorkflowParams,
        agent_ids: Vec<u64>,
    },
    AddAgentToWorkflow {
        workflow_id: u64,
        agent_id: u64,
    },
    CreateRfa {
        workflow_id: u64,
        title: String,
        description: String,
        required_skills: Vec<String>,
    },
    DistributeLeaseFees {
        lease_id: u64,
    },
}
