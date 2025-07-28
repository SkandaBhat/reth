//! Integration tests that creates a MockEthProvider, adds some blocks, and then
//! creates a filter map and verifies that the filter map can be used to query the
//! blocks and verify that the filter map is correct.

use alloy_primitives::{BlockNumber, Log, B256};
use reth_provider::{test_utils::MockEthProvider, ReceiptProvider};
use std::{str::FromStr, sync::Arc};

use crate::indexer::index;
use crate::storage::InMemoryFilterMapsProvider;
use crate::utils::create_test_provider_with_random_blocks_and_receipts;
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

    // get all the logs from the provider
    let receipts = provider.receipts_by_block_range(range.clone()).unwrap_or_default();
    let logs: Vec<Log> = receipts
        .iter()
        .flat_map(|receipt| receipt.iter().flat_map(|r| r.logs.iter()))
        .cloned()
        .collect();

    println!("logs: {:?}", logs.len());

    // find all the logs in the filter map
    for log in logs.iter() {
        let address = log.address.clone();

        let topics: Vec<B256> = log.topics().iter().map(|&topic| topic).collect();

        let logs_result = query_logs(storage.clone(), range.clone(), address, topics.clone());

        assert!(logs_result.is_ok());

        let logs = logs_result.unwrap();

        assert!(
            !logs.is_empty(),
            "Should find at least one matching log, expected log address: {:?}, topics: {:?}",
            log.address,
            topics.len()
        );

        for l in logs {
            assert_eq!(l.address, log.address, "log address mismatch");
            assert_eq!(l.topics(), log.topics(), "log topics mismatch");
        }
    }
}
