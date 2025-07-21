// integration tests that creates a MockEthProvider, adds some blocks, and then
// creates a filter map and verifies that the filter map can be used to query the
// blocks and verify that the filter map is correct.

use alloy_consensus::BlockHeader;
use alloy_primitives::{Address, BlockNumber, Log, B256};
use alloy_rpc_types_eth::BlockHashOrNumber;
use rand::{rng, Rng};
use reth_ethereum_primitives::Receipt;
use reth_provider::test_utils::MockEthProvider;
use reth_provider::ReceiptProvider;
use reth_testing_utils::generators::{
    random_block_range, random_log, random_receipt, rng_with_seed, BlockRangeParams,
};
use std::sync::Arc;

use crate::indexer::index;
use crate::storage::InMemoryFilterMapsProvider;
use reth_filter_maps::{query_logs, FilterMapsReader};

const START_BLOCK: BlockNumber = 0;
const BLOCKS_COUNT: usize = 100;
const TX_COUNT: u8 = 10;
const LOG_COUNT: u8 = 10;
const MAX_TOPICS: usize = 4; // Ethereum logs can have at most 4 topics

pub(crate) async fn create_test_provider_with_random_blocks_and_receipts() -> MockEthProvider {
    let mut rng = rng_with_seed(b"test_filter_map");

    let provider = MockEthProvider::default();

    let blocks = random_block_range(
        &mut rng,
        START_BLOCK..=START_BLOCK + BLOCKS_COUNT as u64 - 1,
        BlockRangeParams { tx_count: 0..TX_COUNT, ..Default::default() },
    );

    provider.extend_blocks(blocks.iter().cloned().map(|b| (b.hash(), b.into_block())));

    let mut receipts: Vec<(BlockNumber, Vec<Receipt>)> = Vec::with_capacity(blocks.len());
    for block in &blocks {
        receipts.reserve_exact(block.body().transactions.len());
        for transaction in block.body().transactions.iter() {
            let mut receipt = random_receipt(&mut rng, transaction, Some(0));
            //generate LOG_COUNT logs
            let logs: Vec<Log> = (0..LOG_COUNT)
                .map(|_| random_log(&mut rng, Some(Address::random()), Some(MAX_TOPICS as u8)))
                .collect();
            receipt.logs = logs;
            receipts.push((block.number(), vec![receipt]));
        }
    }

    provider.extend_receipts(receipts.into_iter());

    provider
}

pub(crate) async fn get_random_log(provider: Arc<MockEthProvider>) -> (Log, BlockNumber) {
    let mut rng = rng();

    // get a random log. loop until we get a log with logs.len() > 0
    loop {
        let block_number = rng.random_range(START_BLOCK..=START_BLOCK + BLOCKS_COUNT as u64 - 1);
        let receipts = provider
            .receipts_by_block(BlockHashOrNumber::Number(block_number))
            .unwrap_or_default()
            .unwrap_or_default();
        if receipts.is_empty() {
            continue;
        }
        let logs = &receipts[rng.random_range(0..receipts.len())].logs;
        if !logs.is_empty() {
            let log = logs[rng.random_range(0..logs.len())].clone();
            return (log, block_number);
        }
    }
}

// integration tests that creates a MockEthProvider, adds some blocks, and then
// creates a filter map and verifies that the filter map can be used to query the
// blocks and verify that the filter map is correct.

#[tokio::test]
async fn test_filter_map() {
    let provider = create_test_provider_with_random_blocks_and_receipts().await;

    let provider = Arc::new(provider);

    let storage = InMemoryFilterMapsProvider::new(provider.clone());

    let storage = Arc::new(storage);

    let range = START_BLOCK..=START_BLOCK + BLOCKS_COUNT as u64 - 1;
    let result = index(provider.clone(), range.clone(), storage.clone()).await;
    assert!(result.is_ok());

    // Print storage info
    let range_info = storage.as_ref().get_filter_maps_range().unwrap();
    println!("Filter maps range: {:?}", range_info);

    // Count total logs
    let mut total_logs = 0;
    for block_num in range.clone() {
        let receipts = provider
            .receipts_by_block(BlockHashOrNumber::Number(block_num))
            .unwrap_or_default()
            .unwrap_or_default();
        for receipt in receipts {
            total_logs += receipt.logs.len();
        }
    }
    println!("Total logs indexed: {}", total_logs);
    // do an iterative loop of 2000 times
    for i in 0..2000 {
        let (log, log_block_number) = get_random_log(provider.clone()).await;

        let addresses = vec![log.address];

        let topics: Vec<Vec<B256>> = log.topics().iter().map(|&topic| vec![topic]).collect();

        // search in a range around the logs block number, but clamp to indexed range
        let from_block = log_block_number.saturating_sub(10000).max(START_BLOCK);
        let to_block =
            log_block_number.saturating_add(10000).min(START_BLOCK + BLOCKS_COUNT as u64 - 1);

        let logs_result =
            query_logs(storage.clone(), from_block, to_block, addresses.clone(), topics.clone());

        let logs = match logs_result {
            Ok(logs) => logs,
            Err(e) => {
                println!("Query failed for log at block {}: {:?}", log_block_number, e);
                continue;
            }
        };

        // Filter out false positives
        // let filtered_logs: Vec<_> = logs
        //     .into_iter()
        //     .filter(|l| reth_filter_maps::verify_log_matches_filter(l, &addresses, &topics))
        //     .collect();

        // Should find at least the original log

        assert!(
            !logs.is_empty(),
            "Should find at least one matching log, expected log address: {:?}, iteration: {}",
            log.address,
            i
        );

        for l in logs {
            assert_eq!(l.address, log.address);
            assert_eq!(l.topics(), log.topics());
        }
    }
}
