//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use crate::{FilterError, FilterResult};
use alloy_primitives::BlockNumber;
use std::{ops::RangeInclusive, vec::Vec};

use crate::storage::types::{
    FilterMapBoundary, FilterMapMetadata, FilterMapRow, FilterMapRowEntry,
    FilterMapsBlockDelimiterEntry, MapIndex, RowIndex,
};

/// Provider trait for reading filter map data.
pub trait FilterMapsReader: Send + Sync {
    /// Get filter map metadata.
    fn get_metadata(&self) -> FilterResult<Option<FilterMapMetadata>>;

    /// Get filter map boundary for a given map index.
    fn get_map_boundary(&self, map_index: MapIndex) -> FilterResult<Option<FilterMapBoundary>>;

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

    /// Find which block contains the given log value index, using binary search.
    /// Returns None if the log index is beyond the indexed range.
    ///
    /// The returned tuple contains the block number and the log value index of the first log in the block.
    fn find_block_for_log_index(&self, log_index: u64) -> FilterResult<Option<(BlockNumber, u64)>> {
        // Check if we are in indexed range
        let metadata = self.get_metadata()?;
        let meta = match metadata {
            Some(meta) => meta,
            None => return Ok(None),
        };

        // Check if log index is beyond the indexed range
        if log_index >= meta.next_log_value_index {
            return Ok(None);
        }

        // Binary search for the block
        let mut start = meta.first_indexed_block;
        let mut end = meta.last_indexed_block;
        let mut result = None;

        println!("start: {:?}, end: {:?}", start, end);

        while start <= end {
            let mid = start + (end - start) / 2;

            // get delimiter for the block
            let delimiter = self.get_block_delimiter(mid)?;
            if let Some(delimiter) = delimiter {
                if log_index >= delimiter.log_value_index {
                    // this block starts before or at our target
                    result = Some((mid, delimiter.log_value_index));
                    println!("found log at index: {:?} searching right for better match", result);
                    start = mid + 1;
                } else {
                    // this block starts after our target
                    end = mid.saturating_sub(1);
                }
            } else {
                // no delimiter found, this shouldnt happen in valid data
                return Err(FilterError::Database("No delimiter found for block".to_string()));
            }
        }

        Ok(result)
    }
    /// Get all map indices that contain logs from the given block range.
    fn get_map_indices_for_block_range(
        &self,
        range: RangeInclusive<BlockNumber>,
    ) -> FilterResult<Vec<MapIndex>> {
        todo!()
    }

    /// Check if a block range is fully indexed.
    fn is_block_range_indexed(&self, range: RangeInclusive<BlockNumber>) -> FilterResult<bool> {
        let metadata = self.get_metadata()?;
        let meta = match metadata {
            Some(meta) => meta,
            None => return Ok(false),
        };
        println!("range: {:?}, meta: {:?}", range, meta);

        Ok(range.start() >= &meta.first_indexed_block && range.end() <= &meta.last_indexed_block)
    }

    /// Get the log value index range for a specific block.
    fn get_log_value_index_range_for_block(
        &self,
        block: BlockNumber,
    ) -> FilterResult<RangeInclusive<u64>> {
        todo!()
    }
}

/// Provider trait for writing filter map data.
pub trait FilterMapsWriter: Send + Sync {
    /// Store filter map metadata.
    fn store_metadata(&self, metadata: FilterMapMetadata) -> FilterResult<()>;

    /// Store filter map boundary for a given map index.
    fn store_map_boundary(
        &self,
        map_index: MapIndex,
        boundary: FilterMapBoundary,
    ) -> FilterResult<()>;

    /// Store filter map rows.
    fn store_filter_map_rows(
        &self,
        map_index: MapIndex,
        rows: Vec<FilterMapRowEntry>,
    ) -> FilterResult<()>;

    /// Store block to log value pointer mapping.
    fn store_block_delimiter(&self, delimiter: FilterMapsBlockDelimiterEntry) -> FilterResult<()>;
}
