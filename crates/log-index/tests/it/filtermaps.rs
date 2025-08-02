//! Integration tests that creates a MockEthProvider, adds some blocks, and then
//! creates a filter map and verifies that the filter map can be used to query the
//! blocks and verify that the filter map is correct.

use alloy_primitives::{BlockNumber, Log, B256};
use alloy_rpc_types_eth::Filter;
use rand::seq::SliceRandom;
use reth_log_index::storage::FilterMapMetadata;
use reth_log_index::FilterMapsWriter;
use reth_provider::{test_utils::MockEthProvider, ReceiptProvider};
use std::sync::Arc;

use crate::filter::get_logs_in_block_range;
use crate::indexer::index;
use crate::storage::InMemoryFilterMapsProvider;
use crate::utils::create_test_provider_with_random_blocks_and_receipts;

const START_BLOCK: BlockNumber = 0;
const BLOCKS_COUNT: usize = 500;
const TX_COUNT: u8 = 150;
const LOG_COUNT: u8 = 1;
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

    println!("provider created");

    let provider = Arc::new(provider);

    let storage = InMemoryFilterMapsProvider::new(provider.clone());

    let storage = Arc::new(storage);

    let range = START_BLOCK..=START_BLOCK + BLOCKS_COUNT as u64 - 1;

    let result = index(provider.clone(), range.clone(), storage.clone()).await;
    assert!(result.is_ok());
    println!("indexed");

    // get all the logs from the provider
    let receipts = provider.receipts_by_block_range(range.clone()).unwrap_or_default();
    let logs: Vec<Log> = receipts
        .iter()
        .flat_map(|receipt| receipt.iter().flat_map(|r| r.logs.iter()))
        .cloned()
        .collect();

    println!("number of logs: {:?}", logs.len());

    // shuffle the logs
    // let mut rng = rand::rng();
    // let mut logs = logs.clone();
    // logs.shuffle(&mut rng);

    // find all the logs in the filter map
    println!("fetching logs");
    for (i, log) in logs.iter().enumerate() {
        if i % 100 == 0 {
            println!("fetching log: {:?}", i);
        }
        let address = log.address.clone();

        let topics: Vec<B256> = log.topics().iter().map(|&topic| topic).collect();

        let mut filter = Filter::new().address(address);

        for (i, topic) in topics.iter().enumerate() {
            filter = match i {
                0 => filter.event_signature(*topic),
                1 => filter.topic1(*topic),
                2 => filter.topic2(*topic),
                3 => filter.topic3(*topic),
                _ => filter,
            };
        }

        let logs = get_logs_in_block_range(
            storage.clone(),
            filter,
            *range.clone().start(),
            *range.clone().end(),
        )
        .await;

        assert!(logs.is_ok());

        let logs = logs.unwrap();

        assert!(
            !logs.is_empty(),
            "Should find at least one matching log, expected log address: {:?}, topics: {:?}",
            log.address,
            topics.len()
        );

        for l in logs.iter() {
            assert_eq!(l.address, log.address, "log address mismatch");
            assert_eq!(l.topics(), log.topics(), "log topics mismatch");
            assert_eq!(l.data, log.data, "log data mismatch");
        }
    }
}
