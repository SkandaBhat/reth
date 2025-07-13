//! Database models for filter maps (EIP-7745).
//!
//! This module contains the data structures that are stored in the database
//! for the filter maps implementation.

use crate::models::IntegerList;
use alloy_primitives::{BlockNumber, B256};
use reth_codecs::Compact;
use serde::{Deserialize, Serialize};

/// Block number to log value index mapping.
///
/// This is stored to enable fast lookup of where a block's logs start
/// in the global log value sequence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Compact)]
pub struct BlockLvPointer {
    /// The block number.
    pub block_number: BlockNumber,
    /// The log value index where this block's logs begin.
    pub lv_index: u64,
}

/// Last block info for a filter map.
///
/// This is stored to track which blocks are contained in each filter map.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Compact)]
pub struct FilterMapLastBlock {
    /// The last block number in the map.
    pub block_number: BlockNumber,
    /// The hash of the last block.
    pub block_hash: B256,
}

/// Metadata about the range of indexed filter maps.
///
/// This tracks the overall state of filter map indexing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Compact)]
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

/// A row in a filter map.
///
/// Each row contains column indices where log values are stored.
/// We use IntegerList for efficient compression of the column indices.
pub type FilterMapRow = IntegerList;