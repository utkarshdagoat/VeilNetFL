use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use near_lake_primitives::AccountId;
#[derive(Clone, Debug)]
pub struct LatestBlockHeight {
    pub account_id: AccountId,
    pub block_height: near_primitives::types::BlockHeight,
}

impl LatestBlockHeight {
    pub fn set(&mut self, block_height: near_primitives::types::BlockHeight) -> &mut Self {
        self.block_height = block_height;
        self
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ModelData {
    pub dataset: String,                // cid for ipfs
    pub compressed_secret_key: Vec<u8>, // the compressed serialized secret for the client
}

#[derive(Default)]
pub struct RequestQueue {
    pub requests: HashSet<ModelData>,
}

impl RequestQueue {
    pub fn add_request(&mut self, request: ModelData) {
        self.requests.insert(request);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct RequestArguments {
    pub epochs: u32,
    pub dataset_cid: String,
    pub compressed_sk: Vec<u8>,
    pub workers: Vec<String>,
}
