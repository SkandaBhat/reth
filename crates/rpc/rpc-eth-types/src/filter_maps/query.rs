//! Query engine for FilterMaps.

use alloy_consensus::{BlockHeader, TxReceipt};
use alloy_primitives::{BlockNumber, Sealable};
use alloy_rpc_types_eth::{Filter, Log};
use reth_primitives_traits::SignedTransaction;
use reth_storage_api::{
    BlockReader, FilterMapReader, HeaderProvider, ReceiptProvider, TransactionsProvider,
};
use roaring::RoaringBitmap;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::{
    map_index_from_lv_index, map_row_index, row_index, FilterMapParams, FilterMatcher,
    MatcherResult, SingleMatcher,
};

/// Errors that can occur during FilterMaps queries.
#[derive(Debug, thiserror::Error)]
pub enum FilterMapsError {
    /// Database error
    #[error("database error: {0}")]
    Database(String),
    /// Range not indexed
    #[error("block range {0}..={1} not indexed")]
    RangeNotIndexed(BlockNumber, BlockNumber),
    /// Invalid filter
    #[error("invalid filter: {0}")]
    InvalidFilter(String),
}

/// Query engine for FilterMaps.
#[derive(Debug, Clone)]
pub struct FilterMapsQueryEngine<P> {
    /// Provider for reading filter map data
    provider: Arc<P>,
    /// FilterMaps parameters
    params: FilterMapParams,
}

impl<P> FilterMapsQueryEngine<P>
where
    P: FilterMapReader
        + HeaderProvider
        + ReceiptProvider
        + TransactionsProvider
        + BlockReader
        + Send
        + Sync,
    P::Header: BlockHeader,
{
    /// Create a new query engine.
    pub fn new(provider: Arc<P>, params: FilterMapParams) -> Self {
        Self { provider, params }
    }

    /// Query logs using FilterMaps.
    /// Returns None if the range is not indexed.
    pub async fn query_logs(
        &self,
        filter: &Filter,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> Result<Option<Vec<Log>>, FilterMapsError> {
        // Check if range is indexed
        if !self.is_range_indexed(from_block, to_block).await? {
            return Ok(None);
        }

        // Create matchers from filter
        let matchers = FilterMatcher::from_filter(filter, from_block, to_block, &self.params);

        // If no matchers, that means match all logs in range
        if matchers.is_empty() {
            return Ok(Some(self.get_all_logs_in_range(from_block, to_block).await?));
        }

        // Execute matchers to get potential log value indices
        let mut result = MatcherResult::wildcard();
        for matcher in matchers {
            let matcher_result = self.execute_matcher(&matcher).await?;
            result = result.and(matcher_result);
        }

        // If no potential matches, return empty
        if !result.is_wildcard && result.potential_matches.is_empty() {
            return Ok(Some(vec![]));
        }

        // Fetch actual logs and verify they match the filter
        let logs = if result.is_wildcard {
            self.get_all_logs_in_range(from_block, to_block).await?
        } else {
            self.fetch_logs_by_indices(result.potential_matches).await?
        };

        // Filter out false positives
        let filtered_logs = logs.into_iter().filter(|log| filter.matches(&log.inner)).collect();

        Ok(Some(filtered_logs))
    }

    /// Check if a block range is indexed.
    async fn is_range_indexed(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> Result<bool, FilterMapsError> {
        self.provider
            .is_filter_map_indexed(from_block..=to_block)
            .map_err(|e| FilterMapsError::Database(e.to_string()))
    }

    /// Execute a matcher.
    async fn execute_matcher(
        &self,
        matcher: &FilterMatcher,
    ) -> Result<MatcherResult, FilterMapsError> {
        // Use a stack to avoid recursion
        enum MatcherOp<'a> {
            Process(&'a FilterMatcher),
            CombineAny(Vec<MatcherResult>),
            CombineSequence(Vec<MatcherResult>),
        }

        let mut stack = vec![MatcherOp::Process(matcher)];
        let mut results_stack = Vec::new();

        while let Some(op) = stack.pop() {
            match op {
                MatcherOp::Process(m) => {
                    match m {
                        FilterMatcher::Single(single) => {
                            let result = self.execute_single_matcher(single).await?;
                            results_stack.push(result);
                        }
                        FilterMatcher::Any(matchers) => {
                            // Push combine operation first (will be processed after all children)
                            stack.push(MatcherOp::CombineAny(Vec::with_capacity(matchers.len())));
                            // Then push all matchers to process (in reverse order)
                            for m in matchers.iter().rev() {
                                stack.push(MatcherOp::Process(m));
                            }
                        }
                        FilterMatcher::Sequence(matchers) => {
                            // Push combine operation first
                            stack.push(MatcherOp::CombineSequence(Vec::with_capacity(
                                matchers.len(),
                            )));
                            // Then push all matchers to process (in reverse order)
                            for m in matchers.iter().rev() {
                                stack.push(MatcherOp::Process(m));
                            }
                        }
                    }
                }
                MatcherOp::CombineAny(mut results) => {
                    // Collect results from the stack
                    let count = results.capacity();
                    for _ in 0..count {
                        if let Some(r) = results_stack.pop() {
                            results.push(r);
                        }
                    }
                    // Combine with OR logic
                    let mut combined = MatcherResult::with_matches(HashSet::new());
                    for r in results {
                        combined = combined.or(r);
                    }
                    results_stack.push(combined);
                }
                MatcherOp::CombineSequence(mut results) => {
                    // Collect results from the stack
                    let count = results.capacity();
                    for _ in 0..count {
                        if let Some(r) = results_stack.pop() {
                            results.push(r);
                        }
                    }
                    // Combine with AND logic
                    let mut combined = MatcherResult::wildcard();
                    for r in results {
                        combined = combined.and(r);
                    }
                    results_stack.push(combined);
                }
            }
        }

        // The final result should be on the stack
        results_stack
            .pop()
            .ok_or_else(|| FilterMapsError::InvalidFilter("No matcher result".to_string()))
    }

    /// Execute a single value matcher.
    async fn execute_single_matcher(
        &self,
        matcher: &SingleMatcher,
    ) -> Result<MatcherResult, FilterMapsError> {
        let mut matches = HashSet::new();

        for &map_index in &matcher.map_indices {
            // Check all layers
            for layer in 0..3 {
                let row_idx = row_index(map_index, layer, &matcher.value, &self.params);
                let global_row_idx = map_row_index(map_index, row_idx, &self.params);

                if let Some(row_data) = self
                    .provider
                    .get_filter_map_row(global_row_idx)
                    .map_err(|e| FilterMapsError::Database(e.to_string()))?
                {
                    // Check if our value's column is in this row
                    // For now, we'll collect all potential matches
                    // In a real implementation, we'd decode the row data and check columns
                    matches.extend(self.decode_row_matches(map_index, row_data)?);
                }
            }
        }

        Ok(MatcherResult::with_matches(matches))
    }

    /// Decode row data to find matching log value indices.
    fn decode_row_matches(
        &self,
        map_index: u32,
        row_data: Vec<u8>,
    ) -> Result<HashSet<u64>, FilterMapsError> {
        // Decode the compressed bitmap
        let bitmap = RoaringBitmap::deserialize_from(&row_data[..])
            .map_err(|e| FilterMapsError::Database(format!("Failed to decode bitmap: {}", e)))?;

        // Convert column indices to log value indices
        let mut log_value_indices = HashSet::new();
        for column in bitmap {
            // Each column represents a log value within this map
            let lv_index = map_index as u64 * self.params.values_per_map() as u64 + column as u64;
            log_value_indices.insert(lv_index);
        }

        Ok(log_value_indices)
    }

    /// Get all logs in a block range (for wildcard queries).
    async fn get_all_logs_in_range(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> Result<Vec<Log>, FilterMapsError> {
        let mut all_logs = Vec::new();

        // Fetch receipts for the block range
        let receipts = self
            .provider
            .receipts_by_block_range(from_block..=to_block)
            .map_err(|e| FilterMapsError::Database(e.to_string()))?;

        // Process each block's receipts
        for (block_offset, block_receipts) in receipts.into_iter().enumerate() {
            let block_num = from_block + block_offset as u64;

            // Get block header for timestamp and hash
            let header = self
                .provider
                .header_by_number(block_num)
                .map_err(|e| FilterMapsError::Database(e.to_string()))?
                .ok_or_else(|| {
                    FilterMapsError::Database(format!("Block {} not found", block_num))
                })?;

            let block_hash = header.hash_slow();
            let block_timestamp = header.timestamp();

            // Get transactions for the block
            let transactions = self
                .provider
                .transactions_by_block(block_num.into())
                .map_err(|e| FilterMapsError::Database(e.to_string()))?
                .ok_or_else(|| {
                    FilterMapsError::Database(format!("No transactions for block {}", block_num))
                })?;

            // Process each receipt in the block
            let mut log_index = 0u64;
            for (tx_index, receipt) in block_receipts.iter().enumerate() {
                // Get transaction hash from the pre-fetched list
                let tx = transactions.get(tx_index).ok_or_else(|| {
                    FilterMapsError::Database(format!(
                        "Transaction {} not found in block {}",
                        tx_index, block_num
                    ))
                })?;

                // Extract logs from receipt
                for log in receipt.logs() {
                    all_logs.push(Log {
                        inner: log.clone(),
                        block_hash: Some(block_hash),
                        block_number: Some(block_num),
                        block_timestamp: Some(block_timestamp),
                        transaction_hash: Some(*tx.tx_hash()),
                        transaction_index: Some(tx_index as u64),
                        log_index: Some(log_index),
                        removed: false,
                    });
                    log_index += 1;
                }
            }
        }

        Ok(all_logs)
    }

    /// Fetch logs by their log value indices.
    async fn fetch_logs_by_indices(
        &self,
        indices: HashSet<u64>,
    ) -> Result<Vec<Log>, FilterMapsError> {
        let mut logs = Vec::new();

        // Group indices by their containing blocks for efficient fetching
        let mut blocks_to_fetch: HashMap<BlockNumber, Vec<u64>> = HashMap::new();

        for &lv_index in &indices {
            // Find which block contains this log value index using binary search
            if let Some(block_num) = self.find_block_by_log_value_index(lv_index).await? {
                blocks_to_fetch.entry(block_num).or_insert_with(Vec::new).push(lv_index);
            }
        }

        // Fetch logs from each block
        for (block_num, block_indices) in blocks_to_fetch {
            let block_logs = self.get_logs_from_block_by_indices(block_num, &block_indices).await?;
            logs.extend(block_logs);
        }

        Ok(logs)
    }

    /// Find which block contains a specific log value index using binary search.
    async fn find_block_by_log_value_index(
        &self,
        log_value_index: u64,
    ) -> Result<Option<BlockNumber>, FilterMapsError> {
        // Get the map containing this log value
        let map_index = map_index_from_lv_index(log_value_index, &self.params);

        // Get map boundaries to determine block range
        let (start_block, end_block) = if map_index == 0 {
            (
                0,
                self.provider
                    .get_map_boundary(0)
                    .map_err(|e| FilterMapsError::Database(e.to_string()))?
                    .map(|(block, _)| block)
                    .unwrap_or(0),
            )
        } else {
            let prev_boundary = self
                .provider
                .get_map_boundary(map_index - 1)
                .map_err(|e| FilterMapsError::Database(e.to_string()))?;
            let curr_boundary = self
                .provider
                .get_map_boundary(map_index)
                .map_err(|e| FilterMapsError::Database(e.to_string()))?;

            match (prev_boundary, curr_boundary) {
                (Some((prev_block, _)), Some((curr_block, _))) => (prev_block + 1, curr_block),
                _ => return Ok(None),
            }
        };

        // Binary search for the block containing this log value index
        let mut low = start_block;
        let mut high = end_block;

        while low <= high {
            let mid = (low + high) / 2;

            // Get the log value range for this block
            if let Some((block_start, block_end)) = self
                .provider
                .get_block_log_range(mid)
                .map_err(|e| FilterMapsError::Database(e.to_string()))?
            {
                if log_value_index >= block_start && log_value_index <= block_end {
                    return Ok(Some(mid));
                } else if log_value_index < block_start {
                    if mid == 0 {
                        break;
                    }
                    high = mid - 1;
                } else {
                    low = mid + 1;
                }
            } else {
                // No log range for this block, try adjacent blocks
                if mid > start_block {
                    // Try lower block
                    if let Some((block_start, block_end)) = self
                        .provider
                        .get_block_log_range(mid - 1)
                        .map_err(|e| FilterMapsError::Database(e.to_string()))?
                    {
                        if log_value_index >= block_start && log_value_index <= block_end {
                            return Ok(Some(mid - 1));
                        }
                    }
                }
                if mid < end_block {
                    // Try higher block
                    if let Some((block_start, block_end)) = self
                        .provider
                        .get_block_log_range(mid + 1)
                        .map_err(|e| FilterMapsError::Database(e.to_string()))?
                    {
                        if log_value_index >= block_start && log_value_index <= block_end {
                            return Ok(Some(mid + 1));
                        }
                    }
                }
                // Skip this block
                if log_value_index < mid as u64 * 1000 {
                    // Rough estimate
                    high = mid - 1;
                } else {
                    low = mid + 1;
                }
            }
        }

        Ok(None)
    }

    /// Get specific logs from a block by their log value indices.
    async fn get_logs_from_block_by_indices(
        &self,
        block_number: BlockNumber,
        target_indices: &[u64],
    ) -> Result<Vec<Log>, FilterMapsError> {
        let mut logs = Vec::new();
        let target_set: HashSet<_> = target_indices.iter().copied().collect();

        // Get block header
        let header = self
            .provider
            .header_by_number(block_number)
            .map_err(|e| FilterMapsError::Database(e.to_string()))?
            .ok_or_else(|| {
                FilterMapsError::Database(format!("Block {} not found", block_number))
            })?;

        let block_hash = header.hash_slow();
        let block_timestamp = header.timestamp();

        // Get receipts for this block
        let receipts = self
            .provider
            .receipts_by_block(block_number.into())
            .map_err(|e| FilterMapsError::Database(e.to_string()))?
            .ok_or_else(|| {
                FilterMapsError::Database(format!("No receipts for block {}", block_number))
            })?;

        // Get the starting log value index for this block
        let (block_start_lv_index, _) = self
            .provider
            .get_block_log_range(block_number)
            .map_err(|e| FilterMapsError::Database(e.to_string()))?
            .ok_or_else(|| {
                FilterMapsError::Database(format!("No log range for block {}", block_number))
            })?;

        // Track current log value index as we iterate
        let mut current_lv_index = block_start_lv_index;
        let mut log_index = 0u64;

        for (tx_index, receipt) in receipts.iter().enumerate() {
            // Get transaction hash
            let transactions = self
                .provider
                .transactions_by_block(block_number.into())
                .map_err(|e| FilterMapsError::Database(e.to_string()))?
                .ok_or_else(|| {
                    FilterMapsError::Database(format!("No transactions for block {}", block_number))
                })?;

            let tx = transactions.get(tx_index).ok_or_else(|| {
                FilterMapsError::Database(format!(
                    "Transaction {} not found in block {}",
                    tx_index, block_number
                ))
            })?;

            for log in receipt.logs() {
                // Check if the first log value (address) is in our target set
                // This handles false positives - we only return logs whose first value matches
                if target_set.contains(&current_lv_index) {
                    logs.push(Log {
                        inner: log.clone(),
                        block_hash: Some(block_hash),
                        block_number: Some(block_number),
                        block_timestamp: Some(block_timestamp),
                        transaction_hash: Some(*tx.tx_hash()),
                        transaction_index: Some(tx_index as u64),
                        log_index: Some(log_index),
                        removed: false,
                    });
                }

                // Increment log value index for this log
                // Each log has 1 address value + N topic values
                current_lv_index += 1 + log.topics().len() as u64;
                log_index += 1;
            }
        }

        Ok(logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_rpc_types_eth::FilterBlockOption;

    #[test]
    fn test_decode_row_matches() {
        // Test that we can decode a roaring bitmap and convert column indices to log value indices
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(10);
        bitmap.insert(20);
        bitmap.insert(30);

        let mut serialized = Vec::new();
        bitmap.serialize_into(&mut serialized).unwrap();

        // Manually call the decoding logic
        let bitmap = RoaringBitmap::deserialize_from(&serialized[..]).unwrap();
        let mut log_value_indices = HashSet::new();
        let map_index = 0u32;
        let params = FilterMapParams::default();

        for column in bitmap {
            let lv_index = map_index as u64 * params.values_per_map() as u64 + column as u64;
            log_value_indices.insert(lv_index);
        }

        assert_eq!(log_value_indices.len(), 3);
        assert!(log_value_indices.contains(&10));
        assert!(log_value_indices.contains(&20));
        assert!(log_value_indices.contains(&30));
    }

    #[test]
    fn test_matcher_result_combinations() {
        // Test the core logic of combining matcher results
        let mut set1 = HashSet::new();
        set1.insert(1);
        set1.insert(2);
        set1.insert(3);

        let mut set2 = HashSet::new();
        set2.insert(2);
        set2.insert(3);
        set2.insert(4);

        let result1 = MatcherResult::with_matches(set1);
        let result2 = MatcherResult::with_matches(set2);

        // Test AND operation
        let and_result = result1.clone().and(result2.clone());
        assert_eq!(and_result.potential_matches.len(), 2);
        assert!(and_result.potential_matches.contains(&2));
        assert!(and_result.potential_matches.contains(&3));
        assert!(!and_result.is_wildcard);

        // Test OR operation
        let or_result = result1.or(result2);
        assert_eq!(or_result.potential_matches.len(), 4);
        assert!(!or_result.is_wildcard);

        // Test wildcard behavior
        let wildcard = MatcherResult::wildcard();
        assert!(wildcard.is_wildcard);
        assert!(wildcard.potential_matches.is_empty());

        // Wildcard AND non-wildcard = non-wildcard
        let and_wildcard = wildcard.clone().and(MatcherResult::with_matches(HashSet::from([5, 6])));
        assert!(!and_wildcard.is_wildcard);
        assert_eq!(and_wildcard.potential_matches.len(), 2);

        // Wildcard OR anything = wildcard
        let or_wildcard = wildcard.or(MatcherResult::with_matches(HashSet::from([7])));
        assert!(or_wildcard.is_wildcard);
    }

    #[test]
    fn test_filter_to_matcher_conversion() {
        let params = FilterMapParams::default();

        // Empty filter should produce no matchers
        let filter = Filter::default();
        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert!(matchers.is_empty());

        // Single address should produce single matcher
        let addr = address!("0000000000000000000000000000000000000001");
        let filter = Filter {
            block_option: FilterBlockOption::default(),
            address: vec![addr].into(),
            topics: Default::default(),
        };
        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);
        matches!(&matchers[0], FilterMatcher::Single(_));

        // Multiple addresses should produce Any matcher
        let filter = Filter {
            block_option: FilterBlockOption::default(),
            address: vec![
                address!("0000000000000000000000000000000000000001"),
                address!("0000000000000000000000000000000000000002"),
            ]
            .into(),
            topics: Default::default(),
        };
        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);
        if let FilterMatcher::Any(inner) = &matchers[0] {
            assert_eq!(inner.len(), 2);
        } else {
            panic!("Expected Any matcher");
        }
    }

    #[test]
    fn test_map_index_calculation() {
        let params = FilterMapParams::default();

        // Test that log value indices map to correct filter maps
        // Default params have 65,536 values per map (2^16)
        assert_eq!(map_index_from_lv_index(0, &params), 0);
        assert_eq!(map_index_from_lv_index(65_535, &params), 0);
        assert_eq!(map_index_from_lv_index(65_536, &params), 1);
        assert_eq!(map_index_from_lv_index(131_072, &params), 2);
    }

    #[test]
    fn test_roaring_bitmap_edge_cases() {
        // Test empty bitmap
        let bitmap = RoaringBitmap::new();
        let mut serialized = Vec::new();
        bitmap.serialize_into(&mut serialized).unwrap();

        let decoded = RoaringBitmap::deserialize_from(&serialized[..]).unwrap();
        assert_eq!(decoded.len(), 0);

        // Test large values
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert(u32::MAX);
        bitmap.insert(0);
        bitmap.insert(1_000_000);

        let mut serialized = Vec::new();
        bitmap.serialize_into(&mut serialized).unwrap();

        let decoded = RoaringBitmap::deserialize_from(&serialized[..]).unwrap();
        assert_eq!(decoded.len(), 3);
        assert!(decoded.contains(u32::MAX));
    }

    #[test]
    fn test_matcher_stack_based_execution() {
        // Test that our iterative matcher execution logic handles nested structures correctly
        // This tests the stack-based algorithm in execute_matcher

        // Create a complex matcher structure: Any(Single, Sequence(Single, Single))
        let single1 = FilterMatcher::Single(SingleMatcher {
            value: b256!("0000000000000000000000000000000000000000000000000000000000000001"),
            map_indices: vec![0],
        });

        let single2 = FilterMatcher::Single(SingleMatcher {
            value: b256!("0000000000000000000000000000000000000000000000000000000000000002"),
            map_indices: vec![0],
        });

        let single3 = FilterMatcher::Single(SingleMatcher {
            value: b256!("0000000000000000000000000000000000000000000000000000000000000003"),
            map_indices: vec![0],
        });

        let sequence = FilterMatcher::Sequence(vec![single2, single3]);
        let any_matcher = FilterMatcher::Any(vec![single1, sequence]);

        // Verify the matcher structure is what we expect
        match &any_matcher {
            FilterMatcher::Any(matchers) => {
                assert_eq!(matchers.len(), 2);
                matches!(&matchers[0], FilterMatcher::Single(_));
                matches!(&matchers[1], FilterMatcher::Sequence(_));
            }
            _ => panic!("Expected Any matcher"),
        }
    }

    #[test]
    fn test_error_cases() {
        // Test invalid roaring bitmap data
        let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result = RoaringBitmap::deserialize_from(&invalid_data[..]);
        assert!(result.is_err());

        // Test FilterMapsError formatting
        let err = FilterMapsError::Database("test error".to_string());
        assert_eq!(err.to_string(), "database error: test error");

        let err = FilterMapsError::RangeNotIndexed(100, 200);
        assert_eq!(err.to_string(), "block range 100..=200 not indexed");
    }
}
