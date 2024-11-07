use crate::types::{LatestBlockHeight, ModelData, RequestArguments, RequestQueue};
use chrono::{DateTime, LocalResult, TimeZone, Utc};
use near_lake_context_derive::LakeContext;
use near_lake_framework::{near_indexer_primitives::types::BlockHeight, LakeBuilder};
use near_lake_primitives::actions::ActionMetaDataExt;
use near_lake_primitives::receipts::ExecutionStatus;
use near_lake_primitives::AccountId;
use std::thread::JoinHandle;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
/// The code is inspired from the chain signature mpc indexer https://github.com/near/mpc/blob/develop/chain-signatures/node/src/indexer.rs

#[derive(Debug, Clone, clap::Parser)]
#[group(id = "indexer-options")]
pub struct Options {
    /// The block height to start indexing from.
    // Defaults to the latest block on 2024-11-07 07:40:22 AM UTC
    #[clap(long, env("INDEXER_START_BLOCK_HEIGHT"), default_value = "178930751")]
    pub start_block_height: u64,

    /// The amount of time before we should that our indexer is behind.
    #[clap(long, env("INDEXER_BEHIND_THRESHOLD"), default_value = "200")]
    pub behind_threshold: u64,

    /// The threshold in seconds to check if the indexer needs to be restarted due to it stalling.
    #[clap(long, env("INDEXER_RUNNING_THRESHOLD"), default_value = "300")]
    pub running_threshold: u64,

    #[clap(subcommand)]
    pub chain_id: ChainId,
}
#[derive(clap::Parser, Debug, Clone)]
pub(crate) enum ChainId {
    Mainnet,
    Testnet,
}

#[derive(Clone)]
pub struct Indexer {
    latest_block_height: Arc<RwLock<LatestBlockHeight>>,
    last_updated_timestamp: Arc<RwLock<Instant>>,
    latest_block_timestamp_nanosec: Arc<RwLock<Option<u64>>>,
    running_threshold: Duration,
    behind_threshold: Duration,
}

impl Indexer {
    pub fn new(latest_block_height: LatestBlockHeight, options: &Options) -> Self {
        tracing::info!(
            "creating new indexer, latest block height: {}",
            latest_block_height.block_height
        );
        Self {
            latest_block_height: Arc::new(RwLock::new(latest_block_height)),
            last_updated_timestamp: Arc::new(RwLock::new(Instant::now())),
            latest_block_timestamp_nanosec: Arc::new(RwLock::new(None)),
            running_threshold: Duration::from_secs(options.running_threshold),
            behind_threshold: Duration::from_secs(options.behind_threshold),
        }
    }
    /// Get the latest block height from the chain.
    pub async fn latest_block_height(&self) -> BlockHeight {
        self.latest_block_height.read().await.block_height
    }

    /// Check whether the indexer is on track with the latest block height from the chain.
    pub async fn is_running(&self) -> bool {
        self.last_updated_timestamp.read().await.elapsed() <= self.running_threshold
    }

    /// Check whether the indexer is behind with the latest block height from the chain.
    pub async fn is_behind(&self) -> bool {
        if let Some(latest_block_timestamp_nanosec) =
            *self.latest_block_timestamp_nanosec.read().await
        {
            is_elapsed_longer_than_timeout(
                latest_block_timestamp_nanosec / 1_000_000_000,
                self.behind_threshold.as_millis() as u64,
            )
        } else {
            true
        }
    }

    async fn update_block_height_and_timestamp(
        &self,
        block_height: BlockHeight,
        block_timestamp_nanosec: u64,
    ) {
        tracing::debug!(block_height, "update_block_height_and_timestamp");
        *self.last_updated_timestamp.write().await = Instant::now();
        *self.latest_block_timestamp_nanosec.write().await = Some(block_timestamp_nanosec);
        let _val = self.latest_block_height.write().await.set(block_height);
    }
}

#[derive(Clone, LakeContext)]
struct Context {
    contract: AccountId,
    worker: AccountId,
    queue: Arc<RwLock<RequestQueue>>,
    indexer: Indexer,
}

pub fn is_elapsed_longer_than_timeout(timestamp_sec: u64, timeout: u64) -> bool {
    if let LocalResult::Single(msg_timestamp) = Utc.timestamp_opt(timestamp_sec as i64, 0) {
        let timeout = Duration::from_millis(timeout);
        let now_datetime: DateTime<Utc> = Utc::now();
        // Calculate the difference in seconds
        let elapsed_duration = now_datetime.signed_duration_since(msg_timestamp);
        let timeout = chrono::Duration::seconds(timeout.as_secs() as i64)
            + chrono::Duration::nanoseconds(timeout.subsec_nanos() as i64);
        elapsed_duration > timeout
    } else {
        false
    }
}
async fn handle_block(
    mut block: near_lake_primitives::block::Block,
    ctx: &Context,
) -> anyhow::Result<()> {
    tracing::debug!(block_height = block.block_height(), "handling block");
    let mut pending_request = Vec::new();
    for action in block.actions().cloned().collect::<Vec<_>>() {
        if action.receiver_id() == ctx.contract {
            tracing::debug!("got action targeting {}", ctx.contract);
            let Some(receipt) = block.receipt_by_id(&action.receipt_id()) else {
                let err = format!(
                    "indexer unable to find block for receipt_id={}",
                    action.receipt_id()
                );
                tracing::warn!("{err}");
                anyhow::bail!(err);
            };
            let ExecutionStatus::SuccessReceiptId(receipt_id) = receipt.status() else {
                continue;
            };
            let Some(function_call) = action.as_function_call() else {
                continue;
            };

            if function_call.method_name() == "add_request" {
                let arguments =
                    match serde_json::from_slice::<'_, RequestArguments>(function_call.args()) {
                        Ok(arguments) => arguments,
                        Err(err) => {
                            tracing::warn!("failed to parse arguments: {err}");
                            continue;
                        }
                    };
                if arguments
                    .workers
                    .iter()
                    .any(|w| ctx.worker == w.parse().unwrap())
                {
                    pending_request.push(ModelData {
                        dataset: arguments.dataset_cid,
                        compressed_secret_key: arguments.compressed_sk,
                    });
                } else {
                    continue;
                }
            }
        }
    }
    ctx.indexer
        .update_block_height_and_timestamp(block.block_height(), block.header().timestamp_nanosec())
        .await;
    let mut queue = ctx.queue.write().await;

    for request in pending_request {
        queue.add_request(request);
    }
    drop(queue);

    let log_indexing_interval = 1000;
    if block.block_height() % log_indexing_interval == 0 {
        tracing::info!(
            "indexed another {} blocks, latest: {}",
            log_indexing_interval,
            block.block_height()
        );
    }

    Ok(())
}

pub fn run(
    options: &Options,
    contract_id: &AccountId,
    worker_account_id: &AccountId,
    queue: &Arc<RwLock<RequestQueue>>,
    rt: &tokio::runtime::Runtime,
) -> anyhow::Result<(JoinHandle<anyhow::Result<()>>, Indexer)> {
    tracing::info!(
        start_block_height = options.start_block_height,
        %contract_id,
        "starting indexer"
    );

    let latest_block_height = rt.block_on(async {
        LatestBlockHeight {
            account_id: worker_account_id.clone(),
            block_height: BlockHeight::from(options.start_block_height),
        }
    });

    let indexer = Indexer::new(latest_block_height, options);
    let context = Context {
        contract: contract_id.clone(),
        worker: worker_account_id.clone(),
        queue: queue.clone(),
        indexer: indexer.clone(),
    };
    let options = options.clone();

    let join_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        let mut i = 0;
        loop {
            if i > 0 {
                tracing::warn!("indexer is behind, restarting count={i}");
            }
            i += 1;
            let Ok(lake) = rt.block_on(async {
                let latest = context.indexer.latest_block_height().await;
                if i > 0 {
                    tracing::warn!("indexer latest height {latest}, restart count={i}");
                }
                let mut lake_builder =
                    LakeBuilder::default().start_block_height(options.start_block_height);
                match options.chain_id {
                    ChainId::Mainnet => {
                        lake_builder = lake_builder.mainnet();
                    }
                    ChainId::Testnet => {
                        lake_builder = lake_builder.testnet();
                    }
                }
                let lake = lake_builder.build()?;
                anyhow::Ok(lake)
            }) else {
                tracing::error!(?options, "indexer failed to build");
                backoff(i, 10, 3000);
                continue;
            };
            let (sender, stream) = near_lake_framework::streamer(lake);
            let join_handle = {
                let context = context.clone();
                rt.spawn(async move { lake.run_with_context(handle_block, &context).await })
            };
            let outcome = rt.block_on(async {
                if i > 0 {
                    tracing::debug!("giving indexer some time to catch up");
                    backoff(i, 10, 300);
                }
                while context.indexer.is_running().await {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    if join_handle.is_finished() {
                        break;
                    }
                }

                // Abort the indexer task if it's still running.
                if !join_handle.is_finished() {
                    tracing::debug!("aborting indexer task");
                    join_handle.abort();
                }

                join_handle.await
            });
            match outcome {
                Ok(Ok(())) => {
                    tracing::warn!("indexer finished successfully? -- this should not happen");
                    break;
                }
                Ok(Err(err)) => {
                    tracing::warn!(%err, "indexer failed");
                }
                Err(err) => {
                    tracing::warn!(%err, "indexer failed");
                }
            }
            backoff(i, 1, 1200)
        }
        Ok(())
    });
    Ok((join_handle, indexer))
}

fn backoff(i: u32, multiplier: u32, max: u64) {
    // Exponential backoff with max delay of max seconds
    let delay: u64 = std::cmp::min(2u64.pow(i).mul(multiplier as u64), max);
    std::thread::sleep(std::time::Duration::from_secs(delay));
}
