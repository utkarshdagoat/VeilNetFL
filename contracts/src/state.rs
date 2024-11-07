use std::collections::{HashMap, HashSet};

use near_sdk::{near, AccountId, NearToken};

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub struct NetworkState {
    pub workers: HashSet<AccountId>,
    pub stake: HashMap<AccountId,NearToken>
}

/// Represents the model training state i.e the workers,the aggregator node
#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub struct RequestsState {
    pub status: ModelStatus,
    pub workers: HashSet<AccountId>,
    pub datasets: HashMap<AccountId, ModelData>, // the key is the publisher account id
    pub model_cid: Vec<String>,
    pub creator: AccountId,
    pub epochs: u32,
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub struct ModelData {
    pub dataset: String,                // cid for ipfs
    pub compressed_secret_key: Vec<u8>, // the compressed serialized secret for the client
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub enum ModelStatus {
    Pending, // The pending state is that it is waiting for the workers to join
    Training,
    Finished,
}

// governance structs for adding workers and removing workers
#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub struct Proposal {
    proposal_id: u32,
    pub proposal_type: ProposalType,
    pub proposar: AccountId,
    pub status: ProposalStatus,
    pub votes: HashMap<AccountId, Vote>,
    pub for_votes: u32,
    pub angaist_votes: u32,
}

impl Proposal {
    pub fn new(
        proposal_id: u32,
        proposal_type: ProposalType,
        proposar: AccountId,
    ) -> Self {
        Self {
            proposal_id,
            proposal_type,
            proposar,
            status: ProposalStatus::Pending,
            votes: HashMap::new(),
            for_votes: 0,
            angaist_votes: 0,
        }
    }
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub enum ProposalType {
    AddWorker(AccountId, NearToken),
    RemoveWorker(AccountId),
    ChangeBaseFee(u128),
    ChangeStakeAmount(u128)
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub enum Vote {
    For,
    Against,
}

#[near(serializers = [json,borsh])]
#[derive(Clone)]
pub struct GovernanceState {
    pub proposals: Vec<Proposal>,
    pub base_fee: u128,
    pub admin: AccountId, // admin is responsible for adding and removing workers
    pub staking_fee: u128,
}
