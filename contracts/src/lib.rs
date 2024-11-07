pub mod state;

use std::collections::{HashMap, HashSet};

// Find all our documentation at https://docs.near.org
use near_sdk::{env, near, require, AccountId, NearToken};
use state::{
    GovernanceState, ModelData, ModelStatus, NetworkState, Proposal, ProposalStatus, ProposalType,
    RequestsState,
};

// Define the contract structure
#[near(contract_state)]
pub struct Contract {
    pub network: NetworkState,
    pub requests: HashMap<u32, RequestsState>,
    pub current_request_id: u32,
    pub governance: GovernanceState,
}

// Define the default, which automatically initializes the contract
impl Default for Contract {
    fn default() -> Self {
        Self {
            network: NetworkState {
                workers: HashSet::new(),
                stake: HashMap::new(),
            },
            requests: HashMap::new(),
            current_request_id: 0,
            governance: GovernanceState {
                proposals: Vec::new(),
                staking_fee: 0,
                admin: "0".parse().unwrap(),
                base_fee: 0,
            },
        }
    }
}

// Implement the contract structure
#[near]
impl Contract {
    #[payable]
    pub fn add_request(
        &mut self,
        epochs: u32,
        dataset_cid: String,
        compressed_sk: Vec<u8>,
        workers: Vec<String>,
    ) {
        let sender = env::predecessor_account_id();
        let fee = env::attached_deposit();
        require!(
            fee >= NearToken::from_near(
                self.governance.base_fee * (epochs as u128) * (workers.len() as u128)
            ),
            "Stake must be greater than staking fee"
        );

        let model_data = ModelData {
            dataset: dataset_cid,
            compressed_secret_key: compressed_sk,
        };
        let mut datasets = HashMap::new();
        datasets.insert(sender.clone(), model_data);

        let workers: HashSet<AccountId> = workers.iter().map(|w| w.parse().unwrap()).collect();

        let request = RequestsState {
            status: ModelStatus::Pending,
            workers,
            datasets,
            model_cid: Vec::new(),
            creator: sender,
            epochs,
        };

        self.requests.insert(self.current_request_id, request);
        self.current_request_id += 1;
    }

    pub fn complete_request(&mut self, request_id: u32, model_cid: String) {

        let request = self.requests.get_mut(&request_id).unwrap();
        let sender = env::predecessor_account_id();
        let is_worker = request.workers.iter().any(|w| w == &sender);
        require!(!is_worker, "Only workers can complete requests");

        (*request).status = ModelStatus::Finished;
        (*request).model_cid.push(model_cid);
    }

    #[payable]
    pub fn add_worker(&mut self) {
        let worker = env::predecessor_account_id();
        let stake = env::attached_deposit();
        require!(
            stake >= NearToken::from_near(self.governance.staking_fee),
            "Stake must be greater than staking fee"
        );

        let proposal = Proposal::new(
            self.governance.proposals.len() as u32,
            ProposalType::AddWorker(worker.clone(), stake),
            worker,
        );

        self.governance.proposals.push(proposal);
    }

    pub fn execute_proposal(&mut self, proposal_id: usize) {
        let sender = env::predecessor_account_id();
        require!(
            sender == self.governance.admin,
            "Only admin can execute proposals"
        );

        let proposal = self.governance.proposals.get_mut(proposal_id).unwrap();
        let accepted = (*proposal).for_votes > (*proposal).angaist_votes;

        if accepted {
            match &proposal.proposal_type {
                ProposalType::AddWorker(worker, stake) => {
                    self.network.workers.insert(worker.clone());
                    self.network.stake.insert(worker.clone(), *stake);
                }
                ProposalType::RemoveWorker(worker) => {
                    self.network.workers.retain(|w| w != worker);
                    self.network.stake.remove(worker);
                    //TODO: Add token burning mechanism maybe distribute through governance?
                }
                ProposalType::ChangeBaseFee(fee) => {
                    self.governance.base_fee = *fee;
                }
                ProposalType::ChangeStakeAmount(stake) => {
                    self.governance.staking_fee = *stake;
                }
            }
            (*proposal).status = ProposalStatus::Approved;
        } else {
            (*proposal).status = ProposalStatus::Rejected;
        }
    }

    pub fn propose_remove_worker(&mut self, worker: String) {
        let sender = env::predecessor_account_id();
        let proposal = Proposal::new(
            self.governance.proposals.len() as u32,
            ProposalType::RemoveWorker(worker.parse().unwrap()),
            sender,
        );
        self.governance.proposals.push(proposal);
    }

    pub fn propose_change_base_fee(&mut self, fee: u128) {
        let sender = env::predecessor_account_id();
        let proposal = Proposal::new(
            self.governance.proposals.len() as u32,
            ProposalType::ChangeBaseFee(fee),
            sender,
        );
        self.governance.proposals.push(proposal);
    }

    pub fn propose_change_stake_amount(&mut self, stake: u128) {
        let sender = env::predecessor_account_id();
        let proposal = Proposal::new(
            self.governance.proposals.len() as u32,
            ProposalType::ChangeStakeAmount(stake),
            sender,
        );
        self.governance.proposals.push(proposal);
    }

    //TODO: Implement the krum function verification
    pub fn verify_krum_and_slash() {}
}

#[cfg(test)]
mod tests {
}
