//! Mock providers for testing filter maps

use crate::{
    constants::DEFAULT_PARAMS,
    params::FilterMapParams,
    utils::{address_value, topic_value},
    FilterMapProvider, FilterRow, RenderedMap,
};
use alloy_primitives::{Address, BlockNumber, Log, B256};
use reth_errors::ProviderResult;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type FilterRowsMap = Arc<Mutex<HashMap<(u32, u32, u32), Vec<FilterRow>>>>;

/// Mock provider for testing query functionality
#[derive(Debug)]
pub struct MockFilterMapProvider {
    filter_rows: FilterRowsMap,
    /// The filter map parameters
    pub params: FilterMapParams,
    block_to_log_index: HashMap<BlockNumber, u64>,
    logs: HashMap<u64, Log>,
}

impl MockFilterMapProvider {
    /// Create a new mock provider
    pub fn new() -> Self {
        Self {
            filter_rows: Arc::new(Mutex::new(HashMap::new())),
            params: DEFAULT_PARAMS.clone(),
            block_to_log_index: HashMap::new(),
            logs: HashMap::new(),
        }
    }

    /// Create a new mock provider with custom params
    pub fn with_params(params: FilterMapParams) -> Self {
        Self {
            filter_rows: Arc::new(Mutex::new(HashMap::new())),
            params,
            block_to_log_index: HashMap::new(),
            logs: HashMap::new(),
        }
    }

    /// Add a block to log index mapping
    pub fn add_block(&mut self, block_number: BlockNumber, log_index: u64) {
        self.block_to_log_index.insert(block_number, log_index);
    }

    /// Add a log at a specific index
    pub fn add_log(&mut self, log_index: u64, log: Log) {
        self.logs.insert(log_index, log);
    }

    /// Add a filter row
    pub fn add_filter_row(&self, map_index: u32, row_index: u32, layer: u32, row: FilterRow) {
        self.filter_rows.lock().unwrap().insert((map_index, row_index, layer), vec![row]);
    }

    /// Add filter rows for an address at specific log indices
    pub fn add_address_rows(&self, address: &Address, log_indices: &[u64]) {
        let addr_value = address_value(address);

        // Group by map index
        let mut map_columns: HashMap<u32, Vec<u64>> = HashMap::new();
        for &log_index in log_indices {
            let map_index = (log_index / self.params.values_per_map()) as u32;
            let col_index = self.params.column_index(log_index, &addr_value);
            map_columns.entry(map_index).or_default().push(col_index);
        }

        // Add rows for each map
        for (map_index, columns) in map_columns {
            let row_index = self.params.row_index(map_index, 0, &addr_value);
            self.add_filter_row(map_index, row_index, 0, columns);
        }
    }

    /// Add filter rows for a topic at specific log indices
    pub fn add_topic_rows(&self, topic: &B256, log_indices: &[u64]) {
        let topic_value = topic_value(topic);

        // Group by map index
        let mut map_columns: HashMap<u32, Vec<u64>> = HashMap::new();
        for &log_index in log_indices {
            let map_index = (log_index / self.params.values_per_map()) as u32;
            let col_index = self.params.column_index(log_index, &topic_value);
            map_columns.entry(map_index).or_default().push(col_index);
        }

        // Add rows for each map
        for (map_index, columns) in map_columns {
            let row_index = self.params.row_index(map_index, 0, &topic_value);
            self.add_filter_row(map_index, row_index, 0, columns);
        }
    }

    /// Store a rendered filter map
    pub fn store_filter_map(&mut self, rendered: &RenderedMap) {
        // Store block pointers
        for (block_num, lv_index) in &rendered.block_lv_pointers {
            self.add_block(*block_num, *lv_index);
        }

        // Store filter rows - directly insert since this represents the complete row
        let mut rows = self.filter_rows.lock().unwrap();
        for (row_idx, row) in rendered.filter_map.iter().enumerate() {
            if !row.is_empty() {
                rows.insert((rendered.map_index, row_idx as u32, 0), vec![row.clone()]);
            }
        }
    }
}

impl Default for MockFilterMapProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterMapProvider for MockFilterMapProvider {
    fn params(&self) -> &FilterMapParams {
        &self.params
    }

    fn block_to_log_index(&self, block_number: BlockNumber) -> ProviderResult<u64> {
        self.block_to_log_index
            .get(&block_number)
            .copied()
            .ok_or_else(|| reth_errors::ProviderError::HeaderNotFound(block_number.into()))
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

/// Simple provider for testing matcher functionality
#[derive(Debug)]
pub struct SimpleProvider {
    /// The filter map parameters
    pub params: FilterMapParams,
    rows: HashMap<(u32, u32, u32), FilterRow>,
}

impl SimpleProvider {
    /// Create a new simple provider
    pub fn new() -> Self {
        Self { params: DEFAULT_PARAMS.clone(), rows: HashMap::new() }
    }

    /// Create a new simple provider with custom params
    pub fn with_params(params: FilterMapParams) -> Self {
        Self { params, rows: HashMap::new() }
    }

    /// Add a row to the provider
    pub fn add_row(&mut self, map_index: u32, row_index: u32, layer: u32, cols: Vec<u64>) {
        self.rows.insert((map_index, row_index, layer), cols);
    }
}

impl Default for SimpleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FilterMapProvider for SimpleProvider {
    fn params(&self) -> &FilterMapParams {
        &self.params
    }

    fn block_to_log_index(&self, _block_number: BlockNumber) -> ProviderResult<u64> {
        Ok(0)
    }

    fn get_filter_rows(
        &self,
        map_indices: &[u32],
        row_index: u32,
        layer: u32,
    ) -> ProviderResult<Vec<FilterRow>> {
        let mut result = Vec::with_capacity(map_indices.len());
        for &map_index in map_indices {
            let row = self.rows.get(&(map_index, row_index, layer)).cloned().unwrap_or_default();
            result.push(row);
        }
        Ok(result)
    }

    fn get_log(&self, _log_index: u64) -> ProviderResult<Option<Log>> {
        Ok(None)
    }
}
