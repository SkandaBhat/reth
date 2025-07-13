//! Filter map processor for orchestrating the construction of filter maps.
//!
//! This module provides the main processor that coordinates the building of filter
//! maps from blocks and logs.

use super::{
    builder::{FilterMapBuilder, LogValueIterator, RenderedMap},
    params::FilterMapParams,
    types::{FilterError, FilterResult},
};
use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::{Log, Receipt};
use std::collections::HashMap;

/// Processes blocks and logs to build filter maps.
#[derive(Debug)]
pub struct FilterMapsProcessor {
    /// Configuration parameters.
    params: FilterMapParams,
    /// Current position iterator.
    iterator: LogValueIterator,
    /// Current map builder.
    builder: FilterMapBuilder,
    /// Completed filter maps.
    finished_maps: Vec<RenderedMap>,
    /// Current range of blocks being indexed.
    indexed_range: (BlockNumber, BlockNumber),
    /// Block hash to number mapping for verification.
    block_hashes: HashMap<BlockNumber, B256>,
}

impl FilterMapsProcessor {
    /// Creates a new processor starting from the given block.
    pub fn new(params: FilterMapParams, start_block: BlockNumber, start_lv_index: u64) -> Self {
        let map_index = (start_lv_index >> params.log_values_per_map) as u32;

        Self {
            params: params.clone(),
            iterator: LogValueIterator::new(start_block, start_lv_index),
            builder: FilterMapBuilder::new(params, map_index),
            finished_maps: Vec::new(),
            indexed_range: (start_block, start_block),
            block_hashes: HashMap::new(),
        }
    }

    /// Processes a single block, adding its logs to the filter maps.
    pub fn process_block(
        &mut self,
        block_number: BlockNumber,
        block_hash: B256,
        receipts: &[Receipt],
    ) -> FilterResult<()> {
        // Verify block number is sequential
        if block_number != self.iterator.block_number {
            return Err(FilterError::InvalidBlockSequence {
                expected: self.iterator.block_number,
                actual: block_number,
            });
        }

        // Store block hash for later reference
        self.block_hashes.insert(block_number, block_hash);
        self.indexed_range.1 = block_number;

        // Process block delimiter if this is the first log of a block
        if self.iterator.is_delimiter {
            // Skip delimiter position
            self.iterator.advance(None);

            // Check if we need to start a new map
            if self.builder.should_finalize(self.iterator.lv_index) {
                self.finalize_current_map()?;
            }
        }

        // Process all logs in the block
        for receipt in receipts {
            for prim_log in &receipt.logs {
                // Convert alloy_primitives::Log to alloy_rpc_types_eth::Log
                let log = Log {
                    inner: prim_log.clone(),
                    block_hash: Some(block_hash),
                    block_number: Some(block_number),
                    block_timestamp: None,
                    transaction_hash: None,
                    transaction_index: None,
                    log_index: None,
                    removed: false,
                };
                self.process_log(&log)?;
            }
        }

        // Move to next block
        self.iterator.next_block();

        Ok(())
    }

    /// Processes a single log.
    fn process_log(&mut self, log: &Log) -> FilterResult<()> {
        // Check if this log would be split across map boundary
        let values_per_map_mask = self.params.values_per_map() - 1;
        if self.iterator.should_skip_to_boundary(log, values_per_map_mask) {
            // Skip to next map boundary
            self.iterator.skip_to_boundary(self.params.values_per_map());

            // Finalize current map and start new one
            self.finalize_current_map()?;
        }

        // Process address value
        if let Some(value) = self.iterator.current_value(log) {
            self.builder.add_log_value(self.iterator.lv_index, value)?;
        }
        self.iterator.advance(Some(log));

        // Process topic values
        for _ in 0..log.topics().len() {
            if let Some(value) = self.iterator.current_value(log) {
                self.builder.add_log_value(self.iterator.lv_index, value)?;
            }
            self.iterator.advance(Some(log));
        }

        Ok(())
    }

    /// Finalizes the current map and starts a new one.
    pub fn finalize_current_map(&mut self) -> FilterResult<Option<RenderedMap>> {
        let map_index = self.builder.map_index();

        // Find first and last blocks in this map
        let first_block = None;
        let last_block = None;
        let block_lv_pointers = Vec::new();

        // TODO: In the real implementation, we would read block lv pointers
        // from storage to determine which blocks are in this map.
        // For now, we'll use a simplified approach.

        // Create rendered map
        let filter_map = std::mem::replace(
            &mut self.builder,
            FilterMapBuilder::new(self.params.clone(), map_index + 1),
        )
        .finalize();

        // If the map is empty, don't store it
        let has_entries = filter_map.iter().any(|row| !row.is_empty());
        if !has_entries {
            return Ok(None);
        }

        let rendered = RenderedMap {
            filter_map,
            map_index,
            first_block: first_block.unwrap_or(self.indexed_range.0),
            last_block: last_block.unwrap_or(self.indexed_range.1),
            last_block_hash: self
                .block_hashes
                .get(&self.indexed_range.1)
                .copied()
                .unwrap_or_default(),
            block_lv_pointers,
        };

        self.finished_maps.push(rendered.clone());
        Ok(Some(rendered))
    }

    /// Returns all completed filter maps.
    pub fn take_finished_maps(&mut self) -> Vec<RenderedMap> {
        std::mem::take(&mut self.finished_maps)
    }

    /// Gets the current log value index.
    pub fn current_lv_index(&self) -> u64 {
        self.iterator.lv_index
    }

    /// Gets the current block number being processed.
    pub fn current_block(&self) -> BlockNumber {
        self.iterator.block_number
    }

    /// Gets the range of blocks that have been indexed.
    pub fn indexed_range(&self) -> (BlockNumber, BlockNumber) {
        self.indexed_range
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256, bytes, LogData};

    fn create_test_log(address: alloy_primitives::Address, topics: Vec<B256>) -> Log {
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

    fn create_test_receipt(logs: Vec<Log>) -> Receipt {
        Receipt { status: true, cumulative_gas_used: 0, logs }
    }

    #[test]
    fn test_processor_single_block() {
        let params = FilterMapParams::default();
        let mut processor = FilterMapsProcessor::new(params, 1, 0);

        // Create a block with one log
        let log = create_test_log(
            address!("1234567890123456789012345678901234567890"),
            vec![b256!("0000000000000000000000000000000000000000000000000000000000000001")],
        );
        let receipt = create_test_receipt(vec![log]);

        // Process the block
        processor.process_block(1, B256::ZERO, &[receipt]).unwrap();

        // Should have processed: delimiter + address + 1 topic = 3 values
        assert_eq!(processor.current_lv_index(), 3);
        assert_eq!(processor.current_block(), 2); // Ready for next block
    }

    #[test]
    fn test_processor_multiple_blocks() {
        let params = FilterMapParams::default();
        let mut processor = FilterMapsProcessor::new(params, 1, 0);

        // Process multiple blocks
        for block_num in 1..=3 {
            let log = create_test_log(
                address!("1234567890123456789012345678901234567890"),
                vec![
                    b256!("0000000000000000000000000000000000000000000000000000000000000001"),
                    b256!("0000000000000000000000000000000000000000000000000000000000000002"),
                ],
            );
            let receipt = create_test_receipt(vec![log]);
            processor.process_block(block_num, B256::ZERO, &[receipt]).unwrap();
        }

        // Each block: delimiter + address + 2 topics = 4 values
        // 3 blocks × 4 values = 12 values total
        assert_eq!(processor.current_lv_index(), 12);
        assert_eq!(processor.indexed_range(), (1, 3));
    }

    #[test]
    fn test_processor_map_finalization() {
        // Use smaller map size for testing
        let mut params = FilterMapParams::default();
        params.log_values_per_map = 4; // 16 values per map

        let mut processor = FilterMapsProcessor::new(params, 1, 0);

        // Process blocks until we exceed map size
        for block_num in 1..=5 {
            let log = create_test_log(
                address!("1234567890123456789012345678901234567890"),
                vec![
                    b256!("0000000000000000000000000000000000000000000000000000000000000001"),
                    b256!("0000000000000000000000000000000000000000000000000000000000000002"),
                ],
            );
            let receipt = create_test_receipt(vec![log]);
            processor.process_block(block_num, B256::ZERO, &[receipt]).unwrap();
        }

        // Should have created at least one map
        let finished = processor.take_finished_maps();
        assert!(!finished.is_empty());
    }
}
