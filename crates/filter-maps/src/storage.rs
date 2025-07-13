//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use alloy_primitives::{BlockNumber, B256};
#[cfg(feature = "reth-codecs")]
use reth_codecs::Compact;
use serde::{Deserialize, Serialize};

/// Block number to log value index mapping.
///
/// This is stored to enable fast lookup of where a block's logs start
/// in the global log value sequence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct BlockLvPointer {
    /// The block number.
    pub block_number: BlockNumber,
    /// The log value index where this block's logs begin.
    pub lv_index: u64,
}

/// Last block info for a filter map.
///
/// This is stored to track which blocks are contained in each filter map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct FilterMapLastBlock {
    /// The last block number in the map.
    pub block_number: BlockNumber,
    /// The hash of the last block.
    pub block_hash: B256,
}

/// Metadata about the range of indexed filter maps.
///
/// This tracks the overall state of filter map indexing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct FilterMapsRange {
    /// Whether the head block has been indexed.
    pub head_indexed: bool,
    /// First block number in the indexed range.
    pub blocks_first: BlockNumber,
    /// One past the last block number in the indexed range.
    pub blocks_after_last: BlockNumber,
    /// The log value index of the head delimiter.
    pub head_delimiter: u64,
    /// First map index in the range.
    pub maps_first: u64,
    /// One past the last map index in the range.
    pub maps_after_last: u64,
    /// Tail partial epoch (for cleanup tracking).
    pub tail_partial_epoch: u64,
    /// Version of the filter maps structure.
    pub version: u8,
}

/// A row in a filter map stored in the database.
///
/// Each row contains column indices where log values are stored.
/// The indices are stored as a sorted list of u64 values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reth-codecs", derive(Compact))]
pub struct StoredFilterMapRow {
    /// The column indices in this row.
    pub columns: Vec<u64>,
}

impl StoredFilterMapRow {
    /// Creates a new filter map row with the given column indices.
    pub const fn new(columns: Vec<u64>) -> Self {
        Self { columns }
    }

    /// Returns true if the row is empty.
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    /// Returns the number of columns in this row.
    pub fn len(&self) -> usize {
        self.columns.len()
    }

    /// Returns an iterator over the column indices.
    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.columns.iter()
    }
}

impl From<Vec<u64>> for StoredFilterMapRow {
    fn from(columns: Vec<u64>) -> Self {
        Self::new(columns)
    }
}

impl From<StoredFilterMapRow> for Vec<u64> {
    fn from(row: StoredFilterMapRow) -> Self {
        row.columns
    }
}

impl From<&[u64]> for StoredFilterMapRow {
    fn from(columns: &[u64]) -> Self {
        Self::new(columns.to_vec())
    }
}
