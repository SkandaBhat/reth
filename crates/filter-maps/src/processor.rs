//! Filter map processor for orchestrating the construction of filter maps.
//!
//! This module provides the main processor that coordinates the building of filter
//! maps from blocks and logs.

use crate::{
    builder::{FilterMapBuilder, LogValueIterator, RenderedMap},
    params::FilterMapParams,
    storage::{FilterMapLastBlock, FilterMapsReader, FilterMapsWriter},
    types::{FilterError, FilterResult},
    FilterMapExt,
};
use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::Log;
use reth_ethereum_primitives::Receipt;
use std::collections::HashMap;

/// Processes blocks and logs to build filter maps.
#[derive(Debug, Clone)]
pub struct FilterMapsProcessor<S: FilterMapsReader + FilterMapsWriter> {
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
    /// Block starting positions in the current map.
    current_map_block_starts: Vec<(BlockNumber, u64)>,
    /// Storage reader and writer.
    storage: S,
}

impl<S: FilterMapsReader + FilterMapsWriter> FilterMapsProcessor<S> {
    /// Creates a new processor starting from the given block.
    pub fn new(params: FilterMapParams, storage: S) -> Self {
        let range = storage.get_filter_maps_range().unwrap_or_default().unwrap_or_default();
        let (start_block, start_lv_index) = (range.blocks_after_last, range.head_delimiter);
        let map_index = (start_lv_index >> params.log_values_per_map) as u32;

        Self {
            params: params.clone(),
            iterator: LogValueIterator::new(start_block, start_lv_index),
            builder: FilterMapBuilder::new(params, map_index),
            finished_maps: Vec::new(),
            indexed_range: (start_block, start_block),
            block_hashes: HashMap::new(),
            current_map_block_starts: Vec::new(),
            storage,
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
            // Record the block starting position (at the delimiter)
            self.current_map_block_starts.push((block_number, self.iterator.lv_index));

            // Store block pointer immediately
            self.storage.store_block_lv_pointer(block_number, self.iterator.lv_index)?;

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

        // Take the block starting positions for this map
        let block_lv_pointers = std::mem::take(&mut self.current_map_block_starts);

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

        // Determine the actual first and last blocks in this map
        let first_block =
            block_lv_pointers.first().map(|&(block, _)| block).unwrap_or(self.indexed_range.0);
        let last_block =
            block_lv_pointers.last().map(|&(block, _)| block).unwrap_or(self.indexed_range.1);

        let rendered = RenderedMap {
            filter_map: filter_map.clone(),
            map_index,
            first_block,
            last_block,
            last_block_hash: self.block_hashes.get(&last_block).copied().unwrap_or_default(),
            block_lv_pointers,
        };

        // Update indexed_range for the next map
        self.indexed_range.0 = self.indexed_range.1 + 1;

        self.finished_maps.push(rendered.clone());
        self.storage.store_filter_map_rows(map_index, filter_map.to_storage_rows())?;
        // Don't store an extra block pointer here - it's already been stored when processing the block
        self.storage.store_filter_map_last_block(
            map_index,
            FilterMapLastBlock {
                block_number: last_block,
                block_hash: self.block_hashes.get(&last_block).copied().unwrap_or_default(),
            },
        )?;
        Ok(Some(rendered))
    }

    /// Returns all completed filter maps.
    pub fn take_finished_maps(&mut self) -> Vec<RenderedMap> {
        std::mem::take(&mut self.finished_maps)
    }

    /// Gets the current log value index.
    pub const fn current_lv_index(&self) -> u64 {
        self.iterator.lv_index
    }

    /// Gets the current block number being processed.
    pub const fn current_block(&self) -> BlockNumber {
        self.iterator.block_number
    }

    /// Gets the range of blocks that have been indexed.
    pub const fn indexed_range(&self) -> (BlockNumber, BlockNumber) {
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

    fn create_test_receipt(logs: Vec<alloy_primitives::Log>) -> Receipt {
        Receipt {
            tx_type: alloy_consensus::TxType::Legacy,
            success: true,
            cumulative_gas_used: 0,
            logs,
        }
    }

    // #[test]
    // fn test_processor_single_block() {
    //     let params = FilterMapParams::default();
    //     let mut processor = FilterMapsProcessor::new(params, 1, 0);

    //     // Create a block with one log
    //     let log = create_test_log(
    //         address!("1234567890123456789012345678901234567890"),
    //         vec![b256!("0000000000000000000000000000000000000000000000000000000000000001")],
    //     );
    //     let receipt = create_test_receipt(vec![log.inner]);

    //     // Process the block
    //     processor.process_block(1, B256::ZERO, &[receipt]).unwrap();

    //     // Should have processed: delimiter + address + 1 topic = 3 values
    //     assert_eq!(processor.current_lv_index(), 3);
    //     assert_eq!(processor.current_block(), 2); // Ready for next block
    // }

    // #[test]
    // fn test_processor_multiple_blocks() {
    //     let params = FilterMapParams::default();
    //     let mut processor = FilterMapsProcessor::new(params, 1, 0);

    //     // Process multiple blocks
    //     for block_num in 1..=3 {
    //         let log = create_test_log(
    //             address!("1234567890123456789012345678901234567890"),
    //             vec![
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000001"),
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000002"),
    //             ],
    //         );
    //         let receipt = create_test_receipt(vec![log.inner]);
    //         processor.process_block(block_num, B256::ZERO, &[receipt]).unwrap();
    //     }

    //     // Each block: delimiter + address + 2 topics = 4 values
    //     // 3 blocks × 4 values = 12 values total
    //     assert_eq!(processor.current_lv_index(), 12);
    //     assert_eq!(processor.indexed_range(), (1, 3));
    // }

    // #[test]
    // fn test_processor_map_finalization() {
    //     // Use smaller map size for testing
    //     let mut params = FilterMapParams {
    //         log_values_per_map: 4, // 16 values per map
    //         ..Default::default()
    //     };
    //     params.log_map_height = 6; // 64 rows instead of default
    //     params.base_row_length_ratio = 4; // Increase row capacity

    //     let mut processor = FilterMapsProcessor::new(params, 1, 0);

    //     // Process blocks until we exceed map size
    //     for block_num in 1..=5 {
    //         let log = create_test_log(
    //             address!("1234567890123456789012345678901234567890"),
    //             vec![
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000001"),
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000002"),
    //             ],
    //         );
    //         let receipt = create_test_receipt(vec![log.inner]);
    //         processor.process_block(block_num, B256::ZERO, &[receipt]).unwrap();
    //     }

    //     // Should have created at least one map
    //     let finished = processor.take_finished_maps();
    //     assert!(!finished.is_empty());

    //     // Verify block_lv_pointers are tracked
    //     let first_map = &finished[0];
    //     assert!(!first_map.block_lv_pointers.is_empty());

    //     // Each block should have a pointer
    //     for (i, &(block_num, lv_index)) in first_map.block_lv_pointers.iter().enumerate() {
    //         // Block numbers should be sequential starting from 1
    //         assert_eq!(block_num, first_map.first_block + i as u64);
    //         // Log value indices should be increasing
    //         if i > 0 {
    //             assert!(lv_index > first_map.block_lv_pointers[i - 1].1);
    //         }
    //     }
    // }

    // #[test]
    // fn test_block_lv_pointers_tracking() {
    //     let params = FilterMapParams::default();
    //     let mut processor = FilterMapsProcessor::new(params, 100, 0);

    //     // Process a few blocks
    //     for block_num in 100..=102 {
    //         let log = create_test_log(
    //             address!("1234567890123456789012345678901234567890"),
    //             vec![b256!("0000000000000000000000000000000000000000000000000000000000000001")],
    //         );
    //         let receipt = create_test_receipt(vec![log.inner]);
    //         processor.process_block(block_num, B256::ZERO, &[receipt]).unwrap();
    //     }

    //     // Force finalize the current map
    //     processor.finalize_current_map().unwrap();
    //     let finished = processor.take_finished_maps();

    //     assert_eq!(finished.len(), 1);
    //     let map = &finished[0];

    //     // Should have 3 block pointers
    //     assert_eq!(map.block_lv_pointers.len(), 3);

    //     // Check specific values
    //     assert_eq!(map.block_lv_pointers[0], (100, 0)); // First block at lv_index 0
    //     assert_eq!(map.block_lv_pointers[1], (101, 3)); // Second block at lv_index 3 (delimiter + address + topic)
    //     assert_eq!(map.block_lv_pointers[2], (102, 6)); // Third block at lv_index 6

    //     // Verify first and last block
    //     assert_eq!(map.first_block, 100);
    //     assert_eq!(map.last_block, 102);
    // }
}
