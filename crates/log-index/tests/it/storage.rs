// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::{map::HashMap, BlockNumber};
use reth_log_index::{
    FilterMapMetadata, FilterMapRow, FilterMapRowEntry, FilterMapsReader, FilterMapsWriter,
    MapIndex, MapRowIndex, RowIndex,
};
use reth_log_index::{FilterMapParams, FilterResult};
use reth_provider::test_utils::MockEthProvider;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct FilterMapsStorage {
    pub(crate) filter_rows: Arc<Mutex<HashMap<MapRowIndex, FilterMapRow>>>,
    pub(crate) block_log_value_indices: Arc<Mutex<HashMap<BlockNumber, u64>>>,
    pub(crate) metadata: Arc<Mutex<Option<FilterMapMetadata>>>,
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

    fn get_filter_map_row(&self, global_row_index: u64) -> FilterResult<Option<FilterMapRow>> {
        let rows = self.storage.filter_rows.lock().unwrap();

        Ok(rows.get(&global_row_index).cloned())
    }

    fn get_log_value_index_for_block(&self, block: BlockNumber) -> FilterResult<Option<u64>> {
        let log_value_index =
            self.storage.block_log_value_indices.lock().unwrap().get(&block).cloned();
        Ok(log_value_index)
    }
}

impl FilterMapsWriter for InMemoryFilterMapsProvider {
    fn store_filter_map_rows(&self, rows: HashMap<MapRowIndex, FilterMapRow>) -> FilterResult<()> {
        let mut filter_rows = self.storage.filter_rows.lock().unwrap();
        for (global_row_index, row) in rows {
            filter_rows.insert(global_row_index, row);
        }
        Ok(())
    }

    fn store_log_value_index_for_block(
        &self,
        block: BlockNumber,
        log_value_index: u64,
    ) -> FilterResult<()> {
        self.storage.block_log_value_indices.lock().unwrap().insert(block, log_value_index);

        Ok(())
    }

    fn store_metadata(&self, metadata: FilterMapMetadata) -> FilterResult<()> {
        let _ = self.storage.metadata.lock().unwrap().insert(metadata);
        Ok(())
    }
}
