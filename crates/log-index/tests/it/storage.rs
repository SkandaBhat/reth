// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::{map::HashMap, BlockNumber, B256};
use alloy_rpc_types_eth::BlockNumHash;
use std::{
    ops::{RangeBounds, RangeInclusive},
    sync::{Arc, Mutex},
};
use tracing::info;

use reth_log_index::{
    BlockBoundary, FilterMapMeta, FilterMapParams, FilterMapsReader, FilterMapsWriter,
    FilterResult, MapLastBlockNumHash, MapValueRows, MAX_LAYERS,
};
use reth_provider::test_utils::MockEthProvider;

#[derive(Clone, Default)]
pub struct FilterMapsStorage {
    pub(crate) rows: Arc<Mutex<HashMap<u64, Vec<u32>>>>,
    pub(crate) block_lv_ptrs: Arc<Mutex<Vec<BlockBoundary>>>,
    pub(crate) metadata: Arc<Mutex<Option<FilterMapMeta>>>,
}

/// In-memory implementation of FilterMapsReader and FilterMapsWriter for testing
#[derive(Clone)]
pub struct InMemoryFilterMapsProvider {
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
    fn get_metadata(&self) -> FilterResult<Option<FilterMapMeta>> {
        Ok(self.storage.metadata.lock().unwrap().clone())
    }

    /// Get rows for specific values in specific maps.
    /// For each (map_index, value) pair, returns the rows across all layers where
    /// that value might be stored, ordered by layer.
    fn get_filter_map_rows(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
        values: &[&B256],
    ) -> FilterResult<Vec<MapValueRows>> {
        let p = &self.params;
        let mut result = Vec::new();

        let log_value_indices = self.get_log_value_indices_range(block_range)?;

        // Convert blocks to maps
        let first_block_lv =
            log_value_indices.first().map(|index| index.log_value_index).unwrap_or(0);
        let last_block_lv =
            log_value_indices.last().map(|index| index.log_value_index).unwrap_or(0);

        let first_map = (first_block_lv >> p.log_values_per_map) as u32;
        let last_map = (last_block_lv >> p.log_values_per_map) as u32;

        let rows = self.storage.rows.lock().unwrap();

        // Process all maps
        for &value in values {
            for current_map in first_map..=last_map {
                let mut map_rows = Vec::new();

                for layer in 0..MAX_LAYERS {
                    // Calculate row_index for THIS specific map and layer
                    let row_index = p.row_index(current_map, layer as u32, value);

                    // Calculate the exact map_row_index for this (map, row) pair
                    let map_row_index = p.map_row_index(current_map, row_index);

                    // Get the row directly from storage
                    let columns =
                        rows.get(&map_row_index).map(|r| r.clone()).unwrap_or_else(Vec::new);

                    if columns.is_empty() {
                        break; // No data at this layer, no higher layers
                    }

                    let columns_len = columns.len();
                    map_rows.push(columns);

                    // Check if we need next layer
                    if columns_len < p.max_row_length(layer as u32) as usize {
                        break;
                    }
                }

                if !map_rows.is_empty() {
                    result.push(MapValueRows {
                        map_index: current_map,
                        value: *value,
                        layers: map_rows,
                    });
                }
            }
        }

        Ok(result)
    }

    fn get_log_value_indices_range(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
    ) -> FilterResult<Vec<BlockBoundary>> {
        let block_lv_ptrs = self.storage.block_lv_ptrs.lock().unwrap();
        let ptrs: Vec<BlockBoundary> = block_lv_ptrs
            .iter()
            .filter(|index| block_range.contains(&index.block_number))
            .cloned()
            .collect();
        Ok(ptrs)
    }

    /// Get the last block info for a given map index
    /// Returns an error if the map hasn't been indexed yet
    fn get_map_last_blocks_range(
        &self,
        from_map: u32,
        to_map: u32,
    ) -> FilterResult<Vec<MapLastBlockNumHash>> {
        Ok(vec![])
    }
}

impl FilterMapsWriter for InMemoryFilterMapsProvider {
    fn store_filter_map_row(&self, map_row_index: u64, row: Vec<u32>) -> FilterResult<()> {
        // Simply store the row directly with its map_row_index as key
        let mut rows = self.storage.rows.lock().unwrap();

        if row.is_empty() {
            // Remove empty rows to save space
            rows.remove(&map_row_index);
        } else {
            rows.insert(map_row_index, row);
        }

        Ok(())
    }

    fn store_log_value_indices_batch(&self, indices: Vec<BlockBoundary>) -> FilterResult<()> {
        let mut block_lv_ptrs = self.storage.block_lv_ptrs.lock().unwrap();
        block_lv_ptrs.extend(indices);
        // Keep sorted by block number for efficient queries
        block_lv_ptrs.sort_by_key(|x| x.block_number);
        Ok(())
    }

    fn store_meta(&self, metadata: FilterMapMeta) -> FilterResult<()> {
        let _ = self.storage.metadata.lock().unwrap().insert(metadata);
        Ok(())
    }

    fn store_map_last_blocks_batch(
        &self,
        last_blocks: Vec<MapLastBlockNumHash>,
    ) -> FilterResult<()> {
        Ok(())
    }
}
