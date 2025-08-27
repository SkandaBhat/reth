//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use crate::types::{BlockBoundary, FilterMapMeta, FilterResult};
use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::BlockNumHash;
use std::ops::RangeBounds;

/// Rows for a specific map and value combination
#[derive(Debug, Clone)]
pub struct MapValueRows {
    /// The map index these rows are for
    pub map_index: u32,
    /// The value hash these rows are for
    pub value: B256,
    /// Rows across all layers for this (map, value) pair
    /// Ordered by layer (0..MAX_LAYERS)
    pub layers: Vec<Vec<u32>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLastBlockNumHash {
    /// The map index
    pub map_index: u32,
    /// The last block number for this map
    pub block_numhash: BlockNumHash,
}

/// Provider trait for reading filter map data.
#[auto_impl::auto_impl(&, Arc)]
pub trait FilterMapsReader: Send + Sync {
    /// Get filter map metadata.
    fn get_metadata(&self) -> FilterResult<Option<FilterMapMeta>>;

    /// Get filter map row for a given map and row index.
    /// Returns None if the row is not found.
    fn get_filter_map_rows(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
        values: &[&B256],
    ) -> FilterResult<Vec<MapValueRows>>;

    /// Get log value indices for a range of blocks.
    /// Returns ordered vec of (block_number, log_value_index).
    fn get_log_value_indices_range(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
    ) -> FilterResult<Vec<BlockBoundary>>;

    /// Get a map's last block number
    fn get_map_last_blocks_range(
        &self,
        from_map: u32,
        to_map: u32,
    ) -> FilterResult<Vec<MapLastBlockNumHash>>;
}

/// Provider trait for writing filter map data.
pub trait FilterMapsWriter: Send + Sync {
    /// Store filter map metadata.
    fn store_meta(&self, metadata: FilterMapMeta) -> FilterResult<()>;

    /// Store filter map rows.
    fn store_filter_map_row(&self, map_row_index: u64, row: Vec<u32>) -> FilterResult<()>;

    /// Batch store log value indices for multiple blocks.
    fn store_log_value_indices_batch(&self, indices: Vec<BlockBoundary>) -> FilterResult<()>;

    /// Batch store last block info for multiple maps.
    fn store_map_last_blocks_batch(
        &self,
        last_blocks: Vec<MapLastBlockNumHash>,
    ) -> FilterResult<()>;
}
