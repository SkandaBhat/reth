#[cfg(test)]
mod tests {
    use crate::filter_maps::{
        constants::DEFAULT_PARAMS,
        matcher::FilterMapProvider,
        params::FilterMapParams,
        query::{address_to_log_value, query_logs, topic_to_log_value},
        types::FilterError,
        FilterRow,
    };
    use alloy_primitives::{address, bytes, BlockNumber, LogData, B256};
    use alloy_rpc_types_eth::Log;
    use reth_errors::ProviderResult;
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    /// Mock provider for testing
    #[derive(Debug)]
    struct MockProvider {
        params: FilterMapParams,
        block_to_log_index: HashMap<BlockNumber, u64>,
        filter_rows: Arc<Mutex<HashMap<(u32, u32, u32), Vec<FilterRow>>>>,
        logs: HashMap<u64, Log>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                params: DEFAULT_PARAMS.clone(),
                block_to_log_index: HashMap::new(),
                filter_rows: Arc::new(Mutex::new(HashMap::new())),
                logs: HashMap::new(),
            }
        }

        fn add_block(&mut self, block_number: BlockNumber, log_index: u64) {
            self.block_to_log_index.insert(block_number, log_index);
        }

        fn add_log(&mut self, log_index: u64, log: Log) {
            self.logs.insert(log_index, log);
        }

        fn add_filter_row(
            &self,
            map_index: u32,
            row_index: u32,
            layer: u32,
            row: FilterRow,
        ) {
            self.filter_rows
                .lock()
                .unwrap()
                .insert((map_index, row_index, layer), vec![row]);
        }
    }

    impl FilterMapProvider for MockProvider {
        fn params(&self) -> &FilterMapParams {
            &self.params
        }

        fn block_to_log_index(&self, block_number: BlockNumber) -> ProviderResult<u64> {
            self.block_to_log_index
                .get(&block_number)
                .copied()
                .ok_or_else(|| {
                    reth_errors::ProviderError::HeaderNotFound(block_number.into())
                })
        }

        fn get_filter_rows(
            &self,
            map_indices: &[u32],
            row_index: u32,
            layer: u32,
        ) -> ProviderResult<Vec<FilterRow>> {
            let rows = self.filter_rows.lock().unwrap();
            let mut result = Vec::with_capacity(map_indices.len());
            
            for &map_index in map_indices {
                let row = rows
                    .get(&(map_index, row_index, layer))
                    .and_then(|v| v.first())
                    .cloned()
                    .unwrap_or_default();
                result.push(row);
            }
            
            Ok(result)
        }

        fn get_log(&self, log_index: u64) -> ProviderResult<Option<Log>> {
            Ok(self.logs.get(&log_index).cloned())
        }
    }

    #[test]
    fn test_query_single_log() {
        let mut provider = MockProvider::new();
        
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
        
        provider.add_log(5, log.clone());
        
        // Add filter rows for the address and topic
        let address_value = address_to_log_value(address);
        let topic_value = topic_to_log_value(topic);
        
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
        let result = query_logs(
            provider_arc,
            0,
            1,
            vec![address],
            vec![vec![topic]],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].address(), address);
        assert_eq!(logs[0].topics()[0], topic);
    }

    #[test]
    fn test_query_match_all_error() {
        let mut provider = MockProvider::new();
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
        let mut provider = MockProvider::new();
        
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
        
        provider.add_log(10, log1.clone());
        provider.add_log(20, log2.clone());
        
        // Add filter rows
        let addr1_value = address_to_log_value(addr1);
        let addr2_value = address_to_log_value(addr2);
        
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
        let result = query_logs(
            provider_arc,
            0,
            0,
            vec![addr1, addr2, addr3],
            vec![],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 2, "Expected 2 logs, got {}", logs.len());
        
        // Check that we got logs for addr1 and addr2
        let addresses: Vec<_> = logs.iter().map(|l| l.address()).collect();
        assert!(addresses.contains(&addr1));
        assert!(addresses.contains(&addr2));
    }

    #[test]
    fn test_query_with_topics() {
        let mut provider = MockProvider::new();
        
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
        
        provider.add_log(50, log.clone());
        
        // Add filter rows
        let map_index = 0u32;
        
        // Address at position 0 (log value 50)
        let addr_value = address_to_log_value(address);
        let addr_row = provider.params.row_index(map_index, 0, &addr_value);
        let addr_col = provider.params.column_index(50, &addr_value);
        provider.add_filter_row(map_index, addr_row, 0, vec![addr_col]);
        
        // Topic1 at position 1 (log value 51)
        let topic1_row = provider.params.row_index(map_index, 0, &topic1);
        let topic1_col = provider.params.column_index(51, &topic1);
        provider.add_filter_row(map_index, topic1_row, 0, vec![topic1_col]);
        
        // Topic2 at position 2 (log value 52)
        let topic2_row = provider.params.row_index(map_index, 0, &topic2);
        let topic2_col = provider.params.column_index(52, &topic2);
        provider.add_filter_row(map_index, topic2_row, 0, vec![topic2_col]);
        
        // Query with specific topics
        let provider_arc = Arc::new(provider);
        let result = query_logs(
            provider_arc,
            0,
            0,
            vec![address],
            vec![
                vec![topic1],           // Must match topic1 at position 0
                vec![topic2, topic3],   // Can match either topic2 or topic3 at position 1
            ],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].topics()[0], topic1);
        assert_eq!(logs[0].topics()[1], topic2);
    }

    #[test]
    fn test_empty_result() {
        let mut provider = MockProvider::new();
        
        provider.add_block(0, 0);
        provider.add_block(1, 100);
        
        let address = address!("0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef");
        
        // No logs added, no filter rows added
        
        let provider_arc = Arc::new(provider);
        let result = query_logs(
            provider_arc,
            0,
            0,
            vec![address],
            vec![],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 0);
    }

    #[test]
    fn test_block_range_filtering() {
        let mut provider = MockProvider::new();
        
        // Blocks: 0 (log 0), 5 (log 50), 10 (log 100)
        provider.add_block(0, 0);
        provider.add_block(5, 50);
        provider.add_block(6, 60);  // Need this for query 0-5
        provider.add_block(10, 100);
        provider.add_block(11, 150);
        
        let address = address!("0x1234567890abcdef1234567890abcdef12345678");
        
        // Add logs in different blocks
        let log1 = Log {
            inner: alloy_primitives::Log {
                address,
                data: LogData::new_unchecked(vec![], bytes!()),
            },
            block_hash: Some(B256::ZERO),
            block_number: Some(2),
            block_timestamp: None,
            transaction_hash: Some(B256::ZERO),
            transaction_index: Some(0),
            log_index: Some(0),
            removed: false,
        };
        
        let log2 = Log {
            inner: alloy_primitives::Log {
                address,
                data: LogData::new_unchecked(vec![], bytes!()),
            },
            block_hash: Some(B256::ZERO),
            block_number: Some(7),
            block_timestamp: None,
            transaction_hash: Some(B256::ZERO),
            transaction_index: Some(0),
            log_index: Some(0),
            removed: false,
        };
        
        provider.add_log(25, log1.clone());  // In block range 0-5
        provider.add_log(75, log2.clone());  // In block range 5-10
        
        // Add filter rows
        let addr_value = address_to_log_value(address);
        
        // Map 0 (indices 0-65535)
        let map0_row = provider.params.row_index(0, 0, &addr_value);
        let map0_col1 = provider.params.column_index(25, &addr_value);
        let map0_col2 = provider.params.column_index(75, &addr_value);
        provider.add_filter_row(0, map0_row, 0, vec![map0_col1, map0_col2]);
        
        let provider_arc = Arc::new(provider);
        
        // Query for blocks 0-5 (should only get log1)
        let result = query_logs(
            provider_arc.clone(),
            0,
            5,
            vec![address],
            vec![],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].block_number, Some(2));
        
        // Query for blocks 5-10 (should only get log2)
        let result = query_logs(
            provider_arc,
            5,
            10,
            vec![address],
            vec![],
        );
        
        assert!(result.is_ok(), "Query failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].block_number, Some(7));
    }

    #[test]
    fn test_log_value_index_calculation() {
        let mut provider = MockProvider::new();
        
        // Set up block to log index mapping
        // Block 0: starts at log value index 0
        // Block 1: starts at log value index 10 (assume 10 log values in block 0)
        // Block 2: starts at log value index 25 (assume 15 log values in block 1)
        provider.add_block(0, 0);   // Block 0 starts at index 0
        provider.add_block(1, 10);  // Block 1 starts at index 10
        provider.add_block(2, 25);  // Block 2 starts at index 25
        provider.add_block(3, 40);  // Block 3 starts at index 40
        
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
        provider.add_log(0, log_block0.clone());   // log stored at address position (index 0)
        provider.add_log(10, log_block1.clone());  // log stored at address position (index 10)
        
        let addr_value = address_to_log_value(address);
        let topic_value = topic_to_log_value(topic);
        
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
            provider.add_filter_row(map_index, addr_row_index, 0, vec![addr_col0, addr_col1, topic_col0, topic_col1]);
        } else {
            // If they map to different rows, add separate filter rows
            provider.add_filter_row(map_index, addr_row_index, 0, vec![addr_col0, addr_col1]);
            provider.add_filter_row(map_index, topic_row_index, 0, vec![topic_col0, topic_col1]);
        }
        
        let provider_arc = Arc::new(provider);
        
        // Test 1: Query block 0 only
        let result = query_logs(
            provider_arc.clone(),
            0,
            0,
            vec![address],
            vec![vec![topic]],
        );
        
        assert!(result.is_ok(), "Query block 0 failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1, "Expected 1 log from block 0");
        assert_eq!(logs[0].block_number, Some(0));
        
        // Test 2: Query block 1 only
        let result = query_logs(
            provider_arc.clone(),
            1,
            1,
            vec![address],
            vec![vec![topic]],
        );
        
        assert!(result.is_ok(), "Query block 1 failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 1, "Expected 1 log from block 1");
        assert_eq!(logs[0].block_number, Some(1));
        
        // Test 3: Query both blocks
        let result = query_logs(
            provider_arc,
            0,
            1,
            vec![address],
            vec![vec![topic]],
        );
        
        assert!(result.is_ok(), "Query both blocks failed: {:?}", result);
        let logs = result.unwrap();
        assert_eq!(logs.len(), 2, "Expected 2 logs from both blocks");
        
        // Verify the logs are in the correct order
        let block_numbers: Vec<_> = logs.iter().map(|l| l.block_number).collect();
        assert!(block_numbers.contains(&Some(0)));
        assert!(block_numbers.contains(&Some(1)));
    }

}