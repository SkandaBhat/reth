//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use crate::FilterResult;
use alloy_primitives::BlockNumber;
use std::vec::Vec;

use crate::storage::types::{
    FilterMapRow, FilterMapRowEntry, FilterMapsBlockDelimiterEntry, MapIndex, RowIndex,
};

/// Provider trait for reading filter map data.
pub trait FilterMapsReader: Send + Sync {
    /// Get filter map rows for given map and row indices.
    fn get_filter_map_rows(
        &self,
        map_index: MapIndex,
        row_indices: &[RowIndex],
    ) -> FilterResult<Vec<FilterMapRow>>;

    /// Get block delimiter for a given block number.
    fn get_block_delimiter(
        &self,
        block: BlockNumber,
    ) -> FilterResult<Option<FilterMapsBlockDelimiterEntry>>;
}

/// Provider trait for writing filter map data.
pub trait FilterMapsWriter: Send + Sync {
    /// Store filter map rows.
    fn store_filter_map_rows(
        &self,
        map_index: MapIndex,
        rows: Vec<FilterMapRowEntry>,
    ) -> FilterResult<()>;

    /// Store block to log value pointer mapping.
    fn store_block_delimiter(&self, delimiter: FilterMapsBlockDelimiterEntry) -> FilterResult<()>;
}
