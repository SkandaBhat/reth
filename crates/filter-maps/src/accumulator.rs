use std::collections::HashMap;

use alloy_primitives::B256;

use crate::{FilterError, FilterMap, FilterMapParams, FilterResult, MAX_LAYERS};

/// Builds filter maps from log data.
#[derive(Debug, Clone)]
pub struct FilterMapAccumulator {
    pub params: FilterMapParams,
    pub current_map: FilterMap,
    pub map_index: u32,
    /// Tracks how many values are in each row at each layer.
    pub row_fill_levels: Vec<HashMap<u32, u32>>,
    /// Cache for row index calculations.
    pub row_cache: HashMap<(u32, B256), u32>,
}

impl FilterMapAccumulator {
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
