//! Filter map processor for orchestrating the construction of filter maps.
//!
//! This module provides the main processor that coordinates the building of filter
//! maps from blocks and logs.

use crate::{
    constants::MAX_LAYERS,
    params::FilterMapParams,
    storage::{FilterMapLastBlock, FilterMapsReader, FilterMapsWriter},
    types::{FilterError, FilterResult},
    utils::{address_value, topic_value},
    FilterMap, FilterMapExt,
};
use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::Log;
use reth_ethereum_primitives::Receipt;
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
    pub const fn new(block_number: BlockNumber, lv_index: u64) -> Self {
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
            log.topics().get(topic_idx).map(topic_value)
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
    pub const fn next_block(&mut self) {
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
    pub const fn skip_to_boundary(&mut self, values_per_map: u64) {
        let next_boundary = ((self.lv_index / values_per_map) + 1) * values_per_map;
        self.lv_index = next_boundary;
    }
}

/// Builds filter maps from log data.
#[derive(Debug, Clone)]
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
    pub const fn should_finalize(&self, lv_index: u64) -> bool {
        lv_index >= (self.map_index as u64 + 1) << self.params.log_values_per_map
    }

    /// Finalizes the current map and returns it.
    pub fn finalize(self) -> FilterMap {
        self.current_map
    }

    /// Gets the current map index.
    pub const fn map_index(&self) -> u32 {
        self.map_index
    }
}

/// Represents a completed filter map with metadata.
#[derive(Debug, Clone, Default)]
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
