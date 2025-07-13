//! Provider traits for filter maps (EIP-7745).
//!
//! This module defines the traits for reading and writing filter map data.

use alloy_primitives::BlockNumber;
use reth_storage_errors::provider::ProviderResult;

// TODO: In the storage PR, import these from reth-db-api when the feature is enabled:
// #[cfg(feature = "db-api")]
// use reth_db_api::models::{FilterMapRow, FilterMapLastBlock, FilterMapsRange};

// For now, use temporary type aliases to define the trait interfaces
/// Temporary type alias for filter map row data.
pub type FilterMapRow = Vec<u32>;

/// Temporary struct for filter map last block info.
#[derive(Debug, Clone)]
pub struct FilterMapLastBlock {
    /// The last block number in the map.
    pub block_number: BlockNumber,
    /// The hash of the last block.
    pub block_hash: alloy_primitives::B256,
}

/// Temporary struct for filter maps metadata.
#[derive(Debug, Clone)]
pub struct FilterMapsRange {
    /// Whether the head block has been indexed.
    pub head_indexed: bool,
    /// First block number in the indexed range.
    pub blocks_first: BlockNumber,
    /// One past the last block number in the indexed range.
    pub blocks_after_last: BlockNumber,
}

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
