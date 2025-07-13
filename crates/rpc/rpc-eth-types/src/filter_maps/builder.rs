//! Filter map construction logic.
//!
//! This module implements the core logic for building filter maps from logs
//! according to EIP-7745.

use super::{
    constants::MAX_LAYERS,
    params::FilterMapParams,
    types::{FilterError, FilterResult},
    utils::{address_value, topic_value},
    FilterMap,
};
use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::Log;
use std::collections::HashMap;

/// Tracks the current position while iterating through logs.
#[derive(Debug, Clone)]
pub struct LogValueIterator {
    /// Current global log value index.
    pub lv_index: u64,
    /// Current block number.
    pub block_number: BlockNumber,
    /// Index of current transaction in block.
    pub tx_index: usize,
    /// Index of current log in transaction.
    pub log_index: usize,
    /// Index of current topic in log (0 = address).
    pub topic_index: usize,
    /// True if we're at a block delimiter position.
    pub is_delimiter: bool,
    /// True if this is the first log of a block.
    pub is_first_in_block: bool,
    /// True if iteration is complete.
    pub finished: bool,
}

impl LogValueIterator {
    /// Creates a new iterator starting at the given position.
    pub fn new(block_number: BlockNumber, lv_index: u64) -> Self {
        Self {
            lv_index,
            block_number,
            tx_index: 0,
            log_index: 0,
            topic_index: 0,
            is_delimiter: true, // Start with delimiter for first block
            is_first_in_block: true,
            finished: false,
        }
    }

    /// Gets the current log value hash if not at a delimiter.
    pub fn current_value(&self, log: &Log) -> Option<B256> {
        if self.is_delimiter {
            return None;
        }

        if self.topic_index == 0 {
            Some(address_value(&log.address()))
        } else {
            let topic_idx = self.topic_index - 1;
            log.topics().get(topic_idx).map(|t| topic_value(t))
        }
    }

    /// Advances to the next log value position.
    pub fn advance(&mut self, current_log: Option<&Log>) -> bool {
        if self.finished {
            return false;
        }

        // Handle delimiter
        if self.is_delimiter {
            self.is_delimiter = false;
            self.lv_index += 1;
            return true;
        }

        // Move to next position
        self.topic_index += 1;
        self.lv_index += 1;

        // Check if we need to move to next log
        if let Some(log) = current_log {
            if self.topic_index > log.topics().len() {
                self.topic_index = 0;
                self.log_index += 1;
                return true;
            }
        }

        true
    }

    /// Moves to the start of the next block.
    pub fn next_block(&mut self) {
        self.block_number += 1;
        self.tx_index = 0;
        self.log_index = 0;
        self.topic_index = 0;
        self.is_delimiter = true;
        self.is_first_in_block = true;
    }

    /// Checks if we should skip to the next map boundary.
    pub fn should_skip_to_boundary(&self, log: &Log, values_per_map_mask: u64) -> bool {
        // Check if this log would be split across a map boundary
        if self.topic_index == 0 {
            let values_needed = 1 + log.topics().len() as u64;
            let remaining_in_map = values_per_map_mask - (self.lv_index & values_per_map_mask) + 1;
            values_needed > remaining_in_map
        } else {
            false
        }
    }

    /// Skips to the next map boundary.
    pub fn skip_to_boundary(&mut self, values_per_map: u64) {
        let next_boundary = ((self.lv_index / values_per_map) + 1) * values_per_map;
        self.lv_index = next_boundary;
    }
}

/// Builds filter maps from log data.
#[derive(Debug)]
pub struct FilterMapBuilder {
    params: FilterMapParams,
    current_map: FilterMap,
    map_index: u32,
    /// Tracks how many values are in each row at each layer.
    row_fill_levels: Vec<HashMap<u32, u32>>,
    /// Cache for row index calculations.
    row_cache: HashMap<(u32, B256), u32>,
}

impl FilterMapBuilder {
    /// Creates a new builder for the specified map index.
    pub fn new(params: FilterMapParams, map_index: u32) -> Self {
        let map_height = params.map_height() as usize;
        let mut row_fill_levels = Vec::with_capacity(MAX_LAYERS as usize);
        for _ in 0..MAX_LAYERS {
            row_fill_levels.push(HashMap::new());
        }

        Self {
            params,
            current_map: vec![vec![]; map_height],
            map_index,
            row_fill_levels,
            row_cache: HashMap::new(),
        }
    }

    /// Adds a log value to the map at the appropriate position.
    pub fn add_log_value(&mut self, lv_index: u64, value: B256) -> FilterResult<()> {
        // Find the appropriate layer
        for layer in 0..MAX_LAYERS {
            let cache_key = (layer, value);
            let row_idx = if let Some(&cached) = self.row_cache.get(&cache_key) {
                cached
            } else {
                let idx = self.params.row_index(self.map_index, layer, &value);
                self.row_cache.insert(cache_key, idx);
                idx
            };

            let max_len = self.params.max_row_length(layer);
            let current_len =
                self.row_fill_levels[layer as usize].get(&row_idx).copied().unwrap_or(0);

            if current_len < max_len {
                // Add to this row
                let col_idx = self.params.column_index(lv_index, &value);
                self.current_map[row_idx as usize].push(col_idx);
                self.row_fill_levels[layer as usize].insert(row_idx, current_len + 1);
                return Ok(());
            }
        }

        Err(FilterError::InsufficientLayers(self.map_index))
    }

    /// Checks if the current map should be finalized.
    pub fn should_finalize(&self, lv_index: u64) -> bool {
        lv_index >= (self.map_index as u64 + 1) << self.params.log_values_per_map
    }

    /// Finalizes the current map and returns it.
    pub fn finalize(self) -> FilterMap {
        self.current_map
    }

    /// Gets the current map index.
    pub fn map_index(&self) -> u32 {
        self.map_index
    }
}

/// Represents a completed filter map with metadata.
#[derive(Debug, Clone)]
pub struct RenderedMap {
    /// The filter map data.
    pub filter_map: FilterMap,
    /// Index of this map.
    pub map_index: u32,
    /// First block that has logs in this map.
    pub first_block: BlockNumber,
    /// Last block that has logs in this map.
    pub last_block: BlockNumber,
    /// Hash of the last block.
    pub last_block_hash: B256,
    /// Log value pointers for blocks starting in this map.
    pub block_lv_pointers: Vec<(BlockNumber, u64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256, bytes, LogData};

    fn create_test_log(address: Address, topics: Vec<B256>) -> Log {
        Log {
            inner: alloy_primitives::Log {
                address,
                data: LogData::new_unchecked(topics, bytes!()),
            },
            block_hash: Some(B256::ZERO),
            block_number: Some(1),
            block_timestamp: None,
            transaction_hash: Some(B256::ZERO),
            transaction_index: Some(0),
            log_index: Some(0),
            removed: false,
        }
    }

    #[test]
    fn test_log_value_iterator() {
        let mut iter = LogValueIterator::new(1, 0);

        // Should start with delimiter
        assert!(iter.is_delimiter);
        assert_eq!(iter.lv_index, 0);

        // Advance past delimiter
        assert!(iter.advance(None));
        assert!(!iter.is_delimiter);
        assert_eq!(iter.lv_index, 1);

        // Create a log with 2 topics
        let log = create_test_log(
            address!("1234567890123456789012345678901234567890"),
            vec![
                b256!("0000000000000000000000000000000000000000000000000000000000000001"),
                b256!("0000000000000000000000000000000000000000000000000000000000000002"),
            ],
        );

        // Should be at address position
        assert_eq!(iter.topic_index, 0);
        let value = iter.current_value(&log);
        assert!(value.is_some());

        // Advance through topics
        assert!(iter.advance(Some(&log)));
        assert_eq!(iter.topic_index, 1);
        assert_eq!(iter.lv_index, 2);

        assert!(iter.advance(Some(&log)));
        assert_eq!(iter.topic_index, 2);
        assert_eq!(iter.lv_index, 3);

        // Should move to next log position
        assert!(iter.advance(Some(&log)));
        assert_eq!(iter.topic_index, 0);
        assert_eq!(iter.log_index, 1);
        assert_eq!(iter.lv_index, 4);
    }

    #[test]
    fn test_filter_map_builder() {
        let params = FilterMapParams::default();
        let mut builder = FilterMapBuilder::new(params.clone(), 0);

        // Add some log values
        let addr_value = address_value(&address!("1234567890123456789012345678901234567890"));
        let topic_value =
            topic_value(&b256!("0000000000000000000000000000000000000000000000000000000000000001"));

        // Add values at different positions
        builder.add_log_value(1, addr_value).unwrap();
        builder.add_log_value(2, topic_value).unwrap();

        // Verify they were added to the map
        let map = builder.finalize();

        // Check that some rows have values
        let non_empty_rows: usize = map.iter().filter(|row| !row.is_empty()).count();
        assert!(non_empty_rows > 0);
    }

    #[test]
    fn test_map_boundary_detection() {
        let params = FilterMapParams::default();
        let values_per_map = params.values_per_map();
        let mut iter = LogValueIterator::new(1, values_per_map - 2);

        // Create a log with 3 topics (4 values total)
        let log = create_test_log(
            address!("1234567890123456789012345678901234567890"),
            vec![
                b256!("0000000000000000000000000000000000000000000000000000000000000001"),
                b256!("0000000000000000000000000000000000000000000000000000000000000002"),
                b256!("0000000000000000000000000000000000000000000000000000000000000003"),
            ],
        );

        // Advance past delimiter
        iter.advance(None);

        // Should detect that this log would cross map boundary
        assert!(iter.should_skip_to_boundary(&log, values_per_map - 1));

        // Skip to boundary
        iter.skip_to_boundary(values_per_map);
        assert_eq!(iter.lv_index, values_per_map);
    }
}
