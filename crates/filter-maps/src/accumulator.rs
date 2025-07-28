use std::collections::{HashMap, VecDeque};

use alloy_primitives::{Address, BlockNumber, B256};

use crate::utils::address_value;
use crate::{
    storage::{FilterMapRow, FilterMapsBlockDelimiterEntry, RowIndex},
    types::BlockDelimiter,
    FilterError, FilterMapParams, FilterResult, MAX_LAYERS,
};
use std::{mem, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterMap {
    pub rows: HashMap<RowIndex, FilterMapRow>,
    pub delimiters: Vec<FilterMapsBlockDelimiterEntry>,
    pub index: u64,
}

impl Default for FilterMap {
    fn default() -> Self {
        Self { rows: HashMap::new(), delimiters: vec![], index: 0 }
    }
}

impl FilterMap {
    pub fn new(index: u64) -> Self {
        Self { rows: HashMap::new(), delimiters: vec![], index }
    }
}

/// Builds filter maps from log data.
#[derive(Debug, Clone)]
pub struct FilterMapAccumulator {
    pub params: FilterMapParams,
    pub current_map: FilterMap,
    pub log_value_index: u64,
    /// Tracks how many values are in each row at each layer.
    pub row_fill_levels: Vec<HashMap<u64, u64>>,
    /// Cache for row index calculations.
    pub row_cache: HashMap<(u64, B256), u64>,
    pub completed_maps: VecDeque<FilterMap>,
}

impl FilterMapAccumulator {
    /// Creates a new builder for the specified map index.
    pub fn new(params: FilterMapParams, map_index: u64, log_value_index: u64) -> Self {
        let mut row_fill_levels = Vec::with_capacity(MAX_LAYERS as usize);
        for _ in 0..MAX_LAYERS {
            row_fill_levels.push(HashMap::new());
        }

        Self {
            params,
            current_map: FilterMap::new(map_index),
            log_value_index,
            row_fill_levels,
            row_cache: HashMap::new(),
            completed_maps: VecDeque::new(),
        }
    }

    /// Adds a block delimiter to the accumulator.
    pub fn add_block_delimiter(&mut self, delimiter: BlockDelimiter) -> FilterResult<()> {
        let delimiter = FilterMapsBlockDelimiterEntry {
            parent_block_hash: delimiter.block_hash,
            parent_block_number: delimiter.block_number,
            parent_block_timestamp: delimiter.timestamp,
            log_value_index: self.log_value_index,
        };
        self.current_map.delimiters.push(delimiter);
        self.log_value_index += 1;
        Ok(())
    }

    /// Adds a log value to the map at the appropriate position.
    pub fn add_log_value(&mut self, value: B256) -> FilterResult<()> {
        if self.should_finalize() {
            // Finalize the current map
            let finished = mem::take(&mut self.current_map);
            self.completed_maps.push_back(finished);

            // Reset for new map
            let map_index = self.log_value_index >> self.params.log_values_per_map;
            self.current_map = FilterMap::new(map_index);
            for row_fill_level in &mut self.row_fill_levels {
                row_fill_level.clear();
            }
            self.row_cache.clear();
        }

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

                self.current_map.rows.entry(row_idx).or_default().columns.push(col_idx);
                self.row_fill_levels[layer as usize].insert(row_idx, current_len + 1);
                // increment log value index
                self.log_value_index += 1;
                return Ok(());
            } else {
                println!(
                    "Max length reached for layer row_idx: {}, current_len: {}, max_len: {}",
                    row_idx, current_len, max_len
                );
            }
        }

        Err(FilterError::MaxLayersExceeded(MAX_LAYERS))
    }

    /// Drains the completed maps.
    pub fn drain_completed_maps(&mut self) -> impl Iterator<Item = FilterMap> + '_ {
        self.completed_maps.drain(..)
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
