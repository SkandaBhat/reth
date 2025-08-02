// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::BlockNumber;
use reth_log_index::storage::{
    FilterMapBoundary, FilterMapMetadata, FilterMapRow, FilterMapRowEntry,
    FilterMapsBlockDelimiterEntry, FilterMapsReader, FilterMapsWriter, MapIndex, MapRowIndex,
    RowIndex,
};
use reth_log_index::{FilterMapParams, FilterResult};
use reth_provider::test_utils::MockEthProvider;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FilterMapsStorage {
    pub(crate) filter_rows: Arc<Mutex<HashMap<MapRowIndex, FilterMapRow>>>,
    pub(crate) block_delimiters: Arc<Mutex<HashMap<BlockNumber, FilterMapsBlockDelimiterEntry>>>,
    pub(crate) metadata: Arc<Mutex<Option<FilterMapMetadata>>>,
    pub(crate) boundaries: Arc<Mutex<HashMap<MapIndex, FilterMapBoundary>>>,
}

/// In-memory implementation of FilterMapsReader and FilterMapsWriter for testing
#[derive(Clone)]
pub(crate) struct InMemoryFilterMapsProvider {
    pub(crate) params: FilterMapParams,
    pub(crate) storage: FilterMapsStorage,
    pub(crate) provider: Arc<MockEthProvider>,
}

impl InMemoryFilterMapsProvider {
    /// Create a new empty provider
    pub(crate) fn new(provider: Arc<MockEthProvider>) -> Self {
        Self { params: FilterMapParams::default(), storage: FilterMapsStorage::default(), provider }
    }
}

impl FilterMapsReader for InMemoryFilterMapsProvider {
    fn get_metadata(&self) -> FilterResult<Option<FilterMapMetadata>> {
        Ok(self.storage.metadata.lock().unwrap().clone())
    }

    fn get_map_boundary(&self, map_index: MapIndex) -> FilterResult<Option<FilterMapBoundary>> {
        Ok(self.storage.boundaries.lock().unwrap().get(&map_index).cloned())
    }

    fn get_filter_map_rows(
        &self,
        map_index: MapIndex,
        row_indices: &[RowIndex],
    ) -> FilterResult<Vec<FilterMapRow>> {
        let rows = self.storage.filter_rows.lock().unwrap();
        let mut result = Vec::new();

        for &row_idx in row_indices {
            let global_row_index = self.params.global_row_index(map_index, row_idx);
            let row = rows
                .get(&global_row_index)
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
        let mut delimiter = self.storage.block_delimiters.lock().unwrap().get(&block).cloned();
        // if there is no delimiter for the block, walk back until we find one
        if delimiter.is_none() && block > 0 {
            let mut block = block.saturating_sub(1);
            while block > 0 {
                delimiter = self.storage.block_delimiters.lock().unwrap().get(&block).cloned();
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
        let mut filter_rows = self.storage.filter_rows.lock().unwrap();
        for (row_idx, row) in rows {
            let global_row_index = self.params.global_row_index(map_index, row_idx);
            filter_rows.insert(global_row_index, row);
        }
        Ok(())
    }

    fn store_block_delimiter(&self, delimiter: FilterMapsBlockDelimiterEntry) -> FilterResult<()> {
        self.storage
            .block_delimiters
            .lock()
            .unwrap()
            .insert(delimiter.parent_block_number, delimiter);

        Ok(())
    }

    fn store_metadata(&self, metadata: FilterMapMetadata) -> FilterResult<()> {
        let _ = self.storage.metadata.lock().unwrap().insert(metadata);
        Ok(())
    }

    fn store_map_boundary(
        &self,
        map_index: MapIndex,
        boundary: FilterMapBoundary,
    ) -> FilterResult<()> {
        self.storage.boundaries.lock().unwrap().insert(map_index, boundary);
        Ok(())
    }
}
