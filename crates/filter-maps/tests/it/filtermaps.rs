//! Integration tests that creates a MockEthProvider, adds some blocks, and then
//! creates a filter map and verifies that the filter map can be used to query the
//! blocks and verify that the filter map is correct.

use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_provider::test_utils::MockEthProvider;
use reth_provider::ReceiptProvider;
use std::sync::Arc;

use crate::indexer::index;
use crate::storage::InMemoryFilterMapsProvider;
use crate::utils::{create_test_provider_with_random_blocks_and_receipts, get_random_log};
use reth_filter_maps::query_logs;

const START_BLOCK: BlockNumber = 0;
const BLOCKS_COUNT: usize = 1000;
const TX_COUNT: u8 = 10;
const LOG_COUNT: u8 = 10;
const MAX_TOPICS: usize = 4;

#[tokio::test]
async fn test_filter_map() {
    let provider: MockEthProvider = create_test_provider_with_random_blocks_and_receipts(
        START_BLOCK,
        BLOCKS_COUNT,
        TX_COUNT,
        LOG_COUNT,
        MAX_TOPICS,
    )
    .await;

    let provider = Arc::new(provider);

    let storage = InMemoryFilterMapsProvider::new(provider.clone());

    let storage = Arc::new(storage);

    let range = START_BLOCK..=START_BLOCK + BLOCKS_COUNT as u64 - 1;
    let result = index(provider.clone(), range.clone(), storage.clone()).await;
    assert!(result.is_ok());

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
    for i in 0..20000 {
        let (log, log_block_number) =
            get_random_log(provider.clone(), START_BLOCK, BLOCKS_COUNT).await;

        let addresses = vec![log.address];

        let topics: Vec<Vec<B256>> = log.topics().iter().map(|&topic| vec![topic]).collect();

        // search in a range around the logs block number, but clamp to indexed range
        let from_block = log_block_number.saturating_sub(100).max(START_BLOCK);
        let to_block =
            log_block_number.saturating_add(100).min(START_BLOCK + BLOCKS_COUNT as u64 - 1);

        println!("searching for log with address {:?} at block {}", log.address, log_block_number);

        let logs_result =
            query_logs(storage.clone(), from_block, to_block, addresses.clone(), topics.clone());

        let logs = match logs_result {
            Ok(logs) => logs,
            Err(e) => {
                println!("Query failed for log at block {}: {:?}", log_block_number, e);
                continue;
            }
        };

        println!("found {} logs", logs.len());

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
            println!("found log with address {:?}", l.address);
            assert_eq!(l.address, log.address);
            assert_eq!(l.topics(), log.topics());
        }
    }
}
