// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::{map::HashMap, BlockNumber};
use reth_log_index::{
    FilterMapMetadata, FilterMapParams, FilterMapRow, FilterMapRowEntry, FilterMapsReader,
    FilterMapsWriter, FilterResult, MapIndex, MapRowIndex, RowIndex,
};
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

    fn get_filter_map_rows(
        &self,
        global_row_indices: Vec<u64>,
    ) -> FilterResult<Option<HashMap<u64, FilterMapRow>>> {
        let rows = self.storage.filter_rows.lock().unwrap();

        Ok(Some(
            global_row_indices.iter().map(|index| rows.get(index).cloned()).flatten().collect(),
        ))
    }

    fn get_log_value_index_for_block(&self, block: BlockNumber) -> FilterResult<Option<u64>> {
        let log_value_index =
            self.storage.block_log_value_indices.lock().unwrap().get(&block).cloned();
        Ok(log_value_index)
    }
}

impl FilterMapsWriter for InMemoryFilterMapsProvider {
    fn store_filter_map_row(
        &self,
        map_row_index: MapRowIndex,
        row: FilterMapRow,
    ) -> FilterResult<()> {
        let mut filter_rows = self.storage.filter_rows.lock().unwrap();
        filter_rows.insert(map_row_index, row);
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
