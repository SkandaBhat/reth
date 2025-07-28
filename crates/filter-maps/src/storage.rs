//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use crate::FilterResult;
use alloy_primitives::{BlockNumber, B256};
#[cfg(feature = "reth-codecs")]
use reth_codecs::Compact;
use serde::{Deserialize, Serialize};
use std::vec::Vec;

/// Row Index within a filter map. (0 to map_height - 1)
pub type RowIndex = u64;

/// Map index identifying a filter map.
pub type MapIndex = u64;

/// A single row entry with its index in a filter map.
pub type FilterMapRowEntry = (RowIndex, FilterMapRow);

/// Block number to log value index mapping.
///
/// This is stored to enable fast lookup of where a block's logs start
/// in the global log value sequence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct FilterMapsBlockDelimiterEntry {
    /// The log value index where this block's logs begin.
    pub log_value_index: u64,
    /// The block number.
    pub parent_block_number: BlockNumber,
    /// Parent block hash.
    pub parent_block_hash: B256,
    /// Parent block timestamp.
    pub parent_block_timestamp: u64,
}

/// A row in a filter map stored in the database.
///
/// Each row contains column indices where log values are stored.
/// The indices are stored as a sorted list of u64 values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct FilterMapRow {
    /// The column indices in this row.
    pub columns: Vec<u64>,
}

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
