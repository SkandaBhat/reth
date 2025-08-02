use alloy_primitives::{map::HashMap, BlockNumber, B256};
use std::collections::VecDeque;

use crate::{
    params::FilterMapParams,
    types::{
        BlockDelimiter, FilterError, FilterMapMetadata, FilterMapRow, FilterResult, LogValue,
        MapRowIndex,
    },
    MAX_LAYERS,
};
use std::mem;

/// A filter map is a collection of rows that contain log values.
/// It is used to store the log values in a way that allows for efficient querying.
/// It is also used to store the block number and log value index for each block.
/// This is used to allow for efficient querying of the log values in a block range.
///
/// The filter map is a 2D array of rows and columns.
/// The rows are the rows of the filter map.
/// The columns are the columns of the filter map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterMap {
    /// The rows of the filter map.
    pub rows: HashMap<MapRowIndex, FilterMapRow>,
    /// The block number and log value index for each block.
    pub block_log_value_indices: HashMap<BlockNumber, u64>, // block number -> log value index
    /// The index of the filter map.
    pub index: u64,
}

impl Default for FilterMap {
    fn default() -> Self {
        Self { rows: HashMap::default(), block_log_value_indices: HashMap::default(), index: 0 }
    }
}

impl FilterMap {
    /// Creates a new filter map.
    pub fn new(index: u64) -> Self {
        Self { rows: HashMap::default(), block_log_value_indices: HashMap::default(), index }
    }

    /// get the last block number
    pub fn last_block_number(&self) -> BlockNumber {
        self.block_log_value_indices.keys().max().copied().unwrap_or(0)
    }

    /// get the map index
    pub fn map_index(&self) -> u64 {
        self.index
    }

    /// get the last log value index
    pub fn last_log_value_index(&self) -> Option<u64> {
        self.block_log_value_indices.values().max().copied()
    }

    /// get the rows
    pub fn rows(&self) -> &HashMap<MapRowIndex, FilterMapRow> {
        &self.rows
    }

    /// get the block log value indices
    pub fn block_log_value_indices(&self) -> &HashMap<BlockNumber, u64> {
        &self.block_log_value_indices
    }
}

/// Builds filter maps from log data.
#[derive(Debug, Clone)]
pub struct FilterMapAccumulator {
    /// The parameters for the filter map.
    pub params: FilterMapParams,
    /// The current filter map.
    pub current_map: FilterMap,
    /// The log value index.
    pub log_value_index: u64,
    /// Tracks how many values are in each row at each layer.
    pub row_fill_levels: Vec<HashMap<u64, u64>>,
    /// Cache for row index calculations.
    pub row_cache: HashMap<(u64, B256), u64>,
    /// The completed filter maps.
    pub completed_maps: VecDeque<FilterMap>,
    /// metadata for the accumulator
    pub metadata: FilterMapMetadata,
}

impl FilterMapAccumulator {
    /// Creates a new builder for the specified map index.
    pub fn new(params: FilterMapParams, metadata: FilterMapMetadata) -> Self {
        let map_index = metadata.next_log_value_index >> params.log_values_per_map;
        let mut row_fill_levels = Vec::with_capacity(MAX_LAYERS as usize);
        for _ in 0..MAX_LAYERS {
            row_fill_levels.push(HashMap::default());
        }

        Self {
            params,
            current_map: FilterMap::new(map_index),
            log_value_index: metadata.next_log_value_index,
            row_fill_levels,
            row_cache: HashMap::default(),
            completed_maps: VecDeque::new(),
            metadata,
        }
    }

    /// get the current log value index
    pub fn log_value_index(&self) -> u64 {
        self.log_value_index
    }

    /// Adds a block delimiter to the accumulator.
    pub fn add_block_delimiter(&mut self, delimiter: BlockDelimiter) -> FilterResult<()> {
        let block_number = delimiter.block_number;
        self.current_map.block_log_value_indices.insert(block_number, self.log_value_index);
        self.log_value_index += 1;
        Ok(())
    }

    /// Adds a log value to the map at the appropriate position.
    pub fn add_log_value(&mut self, value: B256) -> FilterResult<()> {
        // Find the appropriate layer and insert the value
        for layer in 0..MAX_LAYERS {
            let cache_key = (layer, value);
            let row_idx = if let Some(&cached) = self.row_cache.get(&cache_key) {
                cached
            } else {
                let idx = self.params.row_index(self.current_map.index, layer, &value);
                self.row_cache.insert(cache_key, idx);
                idx
            };

            let max_len = self.params.max_row_length(layer);
            let current_len =
                self.row_fill_levels[layer as usize].get(&row_idx).copied().unwrap_or(0);

            if current_len < max_len {
                // Add to this row

                let col_idx = self.params.column_index(self.log_value_index, &value);

                let global_row_index =
                    self.params.global_row_index(self.current_map.index, row_idx);

                self.current_map.rows.entry(global_row_index).or_default().columns.push(col_idx);
                self.row_fill_levels[layer as usize].insert(row_idx, current_len + 1);
                // increment log value index
                self.log_value_index += 1;
                return Ok(());
            }
        }

        Err(FilterError::MaxLayersExceeded(MAX_LAYERS))
    }

    /// Add a single block of log values.
    /// Adds the block delimiter and all log values to the accumulator.
    pub fn add_block(
        &mut self,
        delimiter: BlockDelimiter,
        log_values: Vec<LogValue>,
    ) -> FilterResult<()> {
        self.add_block_delimiter(delimiter)?;
        for log_value in log_values {
            self.add_log_value(log_value.value)?;
        }
        if self.should_finalize() {
            // Finalize the current map
            let finished = mem::take(&mut self.current_map);

            self.metadata.last_indexed_block = finished.last_block_number();
            self.metadata.last_map_index = finished.map_index();
            self.metadata.next_log_value_index = finished.last_log_value_index().unwrap_or(0) + 1;

            self.completed_maps.push_back(finished.clone());

            // Reset for new map
            let map_index = self.log_value_index >> self.params.log_values_per_map;
            self.current_map = FilterMap::new(map_index);
            for row_fill_level in &mut self.row_fill_levels {
                row_fill_level.clear();
            }
            self.row_cache.clear();
        }
        Ok(())
    }

    /// Drains the completed maps.
    pub fn drain_completed_maps(&mut self) -> Vec<FilterMap> {
        self.completed_maps.drain(..).collect()
    }

    /// Returns the current unfinished map, if any.
    pub fn take_partial_map(&mut self) -> Option<(u64, FilterMap)> {
        if self.current_map.rows.values().all(|row| row.columns.is_empty()) {
            None
        } else {
            Some((self.current_map.index, mem::take(&mut self.current_map)))
        }
    }

    /// Checks if the current map should be finalized.
    pub const fn should_finalize(&self) -> bool {
        self.log_value_index
            >= (self.current_map.index as u64 + 1) << self.params.log_values_per_map
    }
}
