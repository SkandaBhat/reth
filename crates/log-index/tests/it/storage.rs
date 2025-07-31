// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::BlockNumber;
use reth_log_index::storage::{
    FilterMapRow, FilterMapRowEntry, FilterMapsBlockDelimiterEntry, FilterMapsReader,
    FilterMapsWriter, MapIndex, RowIndex,
};
use reth_log_index::{FilterMapParams, FilterResult};
use reth_provider::test_utils::MockEthProvider;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

/// In-memory implementation of FilterMapsReader and FilterMapsWriter for testing
#[derive(Clone)]
pub(crate) struct InMemoryFilterMapsProvider {
    pub(crate) params: FilterMapParams,
    filter_rows: Arc<Mutex<HashMap<(MapIndex, RowIndex), FilterMapRow>>>,
    block_delimiters: Arc<Mutex<HashMap<BlockNumber, FilterMapsBlockDelimiterEntry>>>,
    // Additional index for efficient log_index lookups
    log_index_to_block: Arc<Mutex<BTreeMap<u64, BlockNumber>>>,
    pub(crate) provider: Arc<MockEthProvider>,
}

impl InMemoryFilterMapsProvider {
    /// Create a new empty provider
    pub(crate) fn new(provider: Arc<MockEthProvider>) -> Self {
        let mut log_index_to_block = BTreeMap::new();
        log_index_to_block.insert(0, 0); // block 0 starts at log index 0
        Self {
            params: FilterMapParams::default(),
            filter_rows: Arc::new(Mutex::new(HashMap::new())),
            block_delimiters: Arc::new(Mutex::new(HashMap::new())),
            log_index_to_block: Arc::new(Mutex::new(log_index_to_block)),
            provider,
        }
    }

    /// Find the block number containing the given log index
    pub(crate) fn find_block_for_log_index(&self, log_index: u64) -> Option<(BlockNumber, u64)> {
        let index = self.log_index_to_block.lock().unwrap();

        // Use BTreeMap's range operation to find the entry with the largest key <= log_index
        index.range(..=log_index).next_back().map(|(&log_start, &block)| (block, log_start))
    }
}

impl FilterMapsReader for InMemoryFilterMapsProvider {
    fn get_filter_map_rows(
        &self,
        map_index: MapIndex,
        row_indices: &[RowIndex],
    ) -> FilterResult<Vec<FilterMapRow>> {
        let rows = self.filter_rows.lock().unwrap();
        let mut result = Vec::new();

        for &row_idx in row_indices {
            let row = rows
                .get(&(map_index, row_idx))
                .cloned()
                .unwrap_or_else(|| FilterMapRow { columns: Vec::new() });
            result.push(row);
        }

        Ok(result)
    }

    fn get_block_delimiter(
        &self,
        block: BlockNumber,
    ) -> FilterResult<Option<FilterMapsBlockDelimiterEntry>> {
        let mut delimiter = self.block_delimiters.lock().unwrap().get(&block).cloned();
        // if there is no delimiter for the block, walk back until we find one
        if delimiter.is_none() && block > 0 {
            let mut block = block.saturating_sub(1);
            while block > 0 {
                delimiter = self.block_delimiters.lock().unwrap().get(&block).cloned();
                if delimiter.is_some() {
                    break;
                }
                block -= 1;
            }
        }
        Ok(delimiter)
    }
}

impl FilterMapsWriter for InMemoryFilterMapsProvider {
    fn store_filter_map_rows(
        &self,
        map_index: MapIndex,
        rows: Vec<FilterMapRowEntry>,
    ) -> FilterResult<()> {
        let mut filter_rows = self.filter_rows.lock().unwrap();
        for (row_idx, row) in rows {
            filter_rows.insert((map_index, row_idx), row);
        }
        Ok(())
    }

    fn store_block_delimiter(&self, delimiter: FilterMapsBlockDelimiterEntry) -> FilterResult<()> {
        let block_number = delimiter.parent_block_number;
        let log_index = delimiter.log_value_index;

        // Store in both indices
        self.block_delimiters.lock().unwrap().insert(block_number, delimiter);
        self.log_index_to_block.lock().unwrap().insert(log_index, block_number + 1); // +1 because delimiter stores parent block

        Ok(())
    }
}
