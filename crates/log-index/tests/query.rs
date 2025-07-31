//! Query tests for filter maps

#![cfg(feature = "test-utils")]

use alloy_primitives::{address, bytes, LogData, B256};
use alloy_rpc_types_eth::Log;
use reth_log_index::{
    address_value, query_logs, test_utils::MockFilterMapProvider, topic_value, FilterError,
};
use std::sync::Arc;

#[cfg(feature = "test-utils")]
#[test]
fn test_query_single_log() {
    let mut provider = MockFilterMapProvider::new();

    // Setup: Add a single log at index 0 in block 1
    provider.add_block(0, 0);
    provider.add_block(1, 1);
    provider.add_block(2, 10);

    let address = address!("0x1234567890abcdef1234567890abcdef12345678");
    let topic = B256::from([0x42; 32]);

    let log = Log {
        inner: alloy_primitives::Log {
            address,
            data: LogData::new_unchecked(vec![topic], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(1),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    provider.add_log(5, log);

    // Add filter rows for the address and topic
    let address_value = address_value(&address);
    let topic_value = topic_value(&topic);

    // Map 0 contains log indices 0-65535
    let map_index = 0u32;

    // Add rows for address matcher
    let addr_row_index = provider.params.row_index(map_index, 0, &address_value);
    let addr_col_index = provider.params.column_index(5, &address_value);
    provider.add_filter_row(map_index, addr_row_index, 0, vec![addr_col_index]);

    // Add rows for topic matcher
    let topic_row_index = provider.params.row_index(map_index, 0, &topic_value);
    let topic_col_index = provider.params.column_index(6, &topic_value); // log value 6 (position 1)
    provider.add_filter_row(map_index, topic_row_index, 0, vec![topic_col_index]);

    // Query for the log
    let provider_arc = Arc::new(provider);
    let result = query_logs(provider_arc, 0, 1, vec![address], vec![vec![topic]]);

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].address(), address);
    assert_eq!(logs[0].topics()[0], topic);
}

#[test]
fn test_query_match_all_error() {
    let mut provider = MockFilterMapProvider::new();
    provider.add_block(0, 0);
    provider.add_block(101, 1000);

    let provider_arc = Arc::new(provider);

    // Query with no constraints should return MatchAll error
    let result = query_logs(
        provider_arc,
        0,
        100,
        vec![], // No address filter
        vec![], // No topic filter
    );

    assert!(matches!(result, Err(FilterError::MatchAll)));
}

#[test]
fn test_query_multiple_addresses() {
    let mut provider = MockFilterMapProvider::new();

    provider.add_block(0, 0);
    provider.add_block(1, 100);

    let addr1 = address!("0x1111111111111111111111111111111111111111");
    let addr2 = address!("0x2222222222222222222222222222222222222222");
    let addr3 = address!("0x3333333333333333333333333333333333333333");

    // Add logs for addr1 and addr2, but not addr3
    let log1 = Log {
        inner: alloy_primitives::Log {
            address: addr1,
            data: LogData::new_unchecked(vec![], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(0),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    let log2 = Log {
        inner: alloy_primitives::Log {
            address: addr2,
            data: LogData::new_unchecked(vec![], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(0),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(1),
        log_index: Some(1),
        removed: false,
    };

    provider.add_log(10, log1);
    provider.add_log(20, log2);

    // Add filter rows
    let addr1_value = address_value(&addr1);
    let addr2_value = address_value(&addr2);

    let map_index = 0u32;

    // Add row for addr1
    let addr1_row = provider.params.row_index(map_index, 0, &addr1_value);
    let addr1_col = provider.params.column_index(10, &addr1_value);
    provider.add_filter_row(map_index, addr1_row, 0, vec![addr1_col]);

    // Add row for addr2
    let addr2_row = provider.params.row_index(map_index, 0, &addr2_value);
    let addr2_col = provider.params.column_index(20, &addr2_value);
    provider.add_filter_row(map_index, addr2_row, 0, vec![addr2_col]);

    // Query for all three addresses
    let provider_arc = Arc::new(provider);
    let result = query_logs(provider_arc, 0, 0, vec![addr1, addr2, addr3], vec![]);

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2, "Expected 2 logs, got {}", logs.len());

    // Check that we got logs for addr1 and addr2
    let addresses: Vec<_> = logs.iter().map(|l| l.address()).collect();
    assert!(addresses.contains(&addr1));
    assert!(addresses.contains(&addr2));
}

#[test]
fn test_query_with_topics() {
    let mut provider = MockFilterMapProvider::new();

    provider.add_block(0, 0);
    provider.add_block(1, 100);

    let address = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let topic1 = B256::from([0x11; 32]);
    let topic2 = B256::from([0x22; 32]);
    let topic3 = B256::from([0x33; 32]);

    // Create a log with 3 topics
    let log = Log {
        inner: alloy_primitives::Log {
            address,
            data: LogData::new_unchecked(vec![topic1, topic2, topic3], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(0),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    provider.add_log(50, log);

    // Add filter rows
    let map_index = 0u32;

    // Address at position 0 (log value 50)
    let addr_value = address_value(&address);
    let addr_row = provider.params.row_index(map_index, 0, &addr_value);
    let addr_col = provider.params.column_index(50, &addr_value);
    provider.add_filter_row(map_index, addr_row, 0, vec![addr_col]);

    // Topic1 at position 1 (log value 51)
    let topic1_value = topic_value(&topic1);
    let topic1_row = provider.params.row_index(map_index, 0, &topic1_value);
    let topic1_col = provider.params.column_index(51, &topic1_value);
    provider.add_filter_row(map_index, topic1_row, 0, vec![topic1_col]);

    // Topic2 at position 2 (log value 52)
    let topic2_value = topic_value(&topic2);
    let topic2_row = provider.params.row_index(map_index, 0, &topic2_value);
    let topic2_col = provider.params.column_index(52, &topic2_value);
    provider.add_filter_row(map_index, topic2_row, 0, vec![topic2_col]);

    // Query with specific topics
    let provider_arc = Arc::new(provider);
    let result = query_logs(
        provider_arc,
        0,
        0,
        vec![address],
        vec![
            vec![topic1],         // Must match topic1 at position 0
            vec![topic2, topic3], // Can match either topic2 or topic3 at position 1
        ],
    );

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].topics()[0], topic1);
    assert_eq!(logs[0].topics()[1], topic2);
}

#[test]
fn test_empty_result() {
    let mut provider = MockFilterMapProvider::new();

    provider.add_block(0, 0);
    provider.add_block(1, 100);

    let address = address!("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");

    // No logs added, no filter rows added

    let provider_arc = Arc::new(provider);
    let result = query_logs(provider_arc, 0, 0, vec![address], vec![]);

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 0);
}

#[test]
fn test_block_range_filtering() {
    let mut provider = MockFilterMapProvider::new();

    // Blocks: 0 (log 0), 5 (log 50), 10 (log 100)
    provider.add_block(0, 0);
    provider.add_block(5, 50);
    provider.add_block(6, 60); // Need this for query 0-5
    provider.add_block(10, 100);
    provider.add_block(11, 150);

    let address = address!("0x1234567890abcdef1234567890abcdef12345678");

    // Add logs in different blocks
    let log1 = Log {
        inner: alloy_primitives::Log { address, data: LogData::new_unchecked(vec![], bytes!()) },
        block_hash: Some(B256::ZERO),
        block_number: Some(2),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    let log2 = Log {
        inner: alloy_primitives::Log { address, data: LogData::new_unchecked(vec![], bytes!()) },
        block_hash: Some(B256::ZERO),
        block_number: Some(7),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    provider.add_log(25, log1); // In block range 0-5
    provider.add_log(75, log2); // In block range 5-10

    // Add filter rows
    let addr_value = address_value(&address);

    // Map 0 (indices 0-65535)
    let map0_row = provider.params.row_index(0, 0, &addr_value);
    let map0_col1 = provider.params.column_index(25, &addr_value);
    let map0_col2 = provider.params.column_index(75, &addr_value);
    provider.add_filter_row(0, map0_row, 0, vec![map0_col1, map0_col2]);

    let provider_arc = Arc::new(provider);

    // Query for blocks 0-5 (should only get log1)
    let result = query_logs(provider_arc.clone(), 0, 5, vec![address], vec![]);

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].block_number, Some(2));

    // Query for blocks 5-10 (should only get log2)
    let result = query_logs(provider_arc, 5, 10, vec![address], vec![]);

    assert!(result.is_ok(), "Query failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].block_number, Some(7));
}

#[test]
fn test_log_value_index_calculation() {
    let mut provider = MockFilterMapProvider::new();

    // Set up block to log index mapping
    // Block 0: starts at log value index 0
    // Block 1: starts at log value index 10 (assume 10 log values in block 0)
    // Block 2: starts at log value index 25 (assume 15 log values in block 1)
    provider.add_block(0, 0); // Block 0 starts at index 0
    provider.add_block(1, 10); // Block 1 starts at index 10
    provider.add_block(2, 25); // Block 2 starts at index 25
    provider.add_block(3, 40); // Block 3 starts at index 40

    let address = address!("0x1111111111111111111111111111111111111111");
    let topic = B256::from([0xaa; 32]);

    // Create logs with different numbers of topics to test log value index calculation
    let log_block0 = Log {
        inner: alloy_primitives::Log {
            address,
            data: LogData::new_unchecked(vec![topic], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(0),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    let log_block1 = Log {
        inner: alloy_primitives::Log {
            address,
            data: LogData::new_unchecked(vec![topic], bytes!()),
        },
        block_hash: Some(B256::ZERO),
        block_number: Some(1),
        block_timestamp: None,
        transaction_hash: Some(B256::ZERO),
        transaction_index: Some(0),
        log_index: Some(0),
        removed: false,
    };

    // In FilterMaps, each log generates multiple log values:
    // - 1 for the address (at log value index N)
    // - 1 for each topic (at log value index N+1, N+2, etc.)
    //
    // For our test:
    // Block 0, log 0: address at index 0, topic at index 1
    // Block 1, log 0: address at index 10, topic at index 11

    // Add the logs at their calculated positions
    provider.add_log(0, log_block0); // log stored at address position (index 0)
    provider.add_log(10, log_block1); // log stored at address position (index 10)

    let addr_value = address_value(&address);
    let topic_value = topic_value(&topic);

    // Add filter rows for address and topic matches
    let map_index = 0u32;

    // Build filter rows for address matches
    let addr_row_index = provider.params.row_index(map_index, 0, &addr_value);
    let addr_col0 = provider.params.column_index(0, &addr_value);
    let addr_col1 = provider.params.column_index(10, &addr_value);

    // Build filter rows for topic matches
    let topic_row_index = provider.params.row_index(map_index, 0, &topic_value);
    let topic_col0 = provider.params.column_index(1, &topic_value);
    let topic_col1 = provider.params.column_index(11, &topic_value);

    // Check if address and topic values map to the same row index
    if addr_row_index == topic_row_index {
        // If they map to the same row, combine all columns into one filter row
        provider.add_filter_row(
            map_index,
            addr_row_index,
            0,
            vec![addr_col0, addr_col1, topic_col0, topic_col1],
        );
    } else {
        // If they map to different rows, add separate filter rows
        provider.add_filter_row(map_index, addr_row_index, 0, vec![addr_col0, addr_col1]);
        provider.add_filter_row(map_index, topic_row_index, 0, vec![topic_col0, topic_col1]);
    }

    let provider_arc = Arc::new(provider);

    // Test 1: Query block 0 only
    let result = query_logs(provider_arc.clone(), 0, 0, vec![address], vec![vec![topic]]);

    assert!(result.is_ok(), "Query block 0 failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1, "Expected 1 log from block 0");
    assert_eq!(logs[0].block_number, Some(0));

    // Test 2: Query block 1 only
    let result = query_logs(provider_arc.clone(), 1, 1, vec![address], vec![vec![topic]]);

    assert!(result.is_ok(), "Query block 1 failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 1, "Expected 1 log from block 1");
    assert_eq!(logs[0].block_number, Some(1));

    // Test 3: Query both blocks
    let result = query_logs(provider_arc, 0, 1, vec![address], vec![vec![topic]]);

    assert!(result.is_ok(), "Query both blocks failed: {result:?}");
    let logs = result.unwrap();
    assert_eq!(logs.len(), 2, "Expected 2 logs from both blocks");

    // Verify the logs are in the correct order
    let block_numbers: Vec<_> = logs.iter().map(|l| l.block_number).collect();
    assert!(block_numbers.contains(&Some(0)));
    assert!(block_numbers.contains(&Some(1)));
}
