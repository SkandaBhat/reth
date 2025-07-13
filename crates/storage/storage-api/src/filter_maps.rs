//! Provider traits for filter maps (EIP-7745).
//!
//! This module defines the traits for reading and writing filter map data.

use alloc::vec::Vec;
use alloy_primitives::BlockNumber;
use reth_filter_maps::storage::{FilterMapLastBlock, FilterMapsRange, StoredFilterMapRow};
use reth_storage_errors::provider::ProviderResult;

/// Type alias for filter map rows.
///
/// For the provider traits, we use `StoredFilterMapRow` directly.
pub type FilterMapRow = StoredFilterMapRow;

/// Provider trait for reading filter map data.
pub trait FilterMapsReader: Send + Sync {
    /// Get filter map rows for given map and row indices.
    fn get_filter_map_rows(
        &self,
        map_index: u32,
        row_indices: &[u32],
    ) -> ProviderResult<Vec<FilterMapRow>>;

    /// Get block to log value pointer.
    fn get_block_lv_pointer(&self, block: BlockNumber) -> ProviderResult<Option<u64>>;

    /// Get the last block info for a specific filter map.
    fn get_filter_map_last_block(
        &self,
        map_index: u32,
    ) -> ProviderResult<Option<FilterMapLastBlock>>;

    /// Get the filter maps metadata.
    fn get_filter_maps_range(&self) -> ProviderResult<Option<FilterMapsRange>>;

    // TODO: Implement in storage PR
}

/// Provider trait for writing filter map data.
pub trait FilterMapsWriter: Send + Sync {
    /// Store filter map rows.
    fn store_filter_map_rows(
        &self,
        map_index: u32,
        rows: Vec<(u32, FilterMapRow)>,
    ) -> ProviderResult<()>;

    /// Store block to log value pointer mapping.
    fn store_block_lv_pointer(&self, block: BlockNumber, lv_pointer: u64) -> ProviderResult<()>;

    /// Store the last block info for a filter map.
    fn store_filter_map_last_block(
        &self,
        map_index: u32,
        last_block: FilterMapLastBlock,
    ) -> ProviderResult<()>;

    /// Update the filter maps metadata.
    fn update_filter_maps_range(&self, range: FilterMapsRange) -> ProviderResult<()>;

    // TODO: Implement in storage PR
}
