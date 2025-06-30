//! Filter map storage traits for efficient log queries based on EIP-7745.

use alloc::vec::Vec;
use alloy_primitives::{Address, BlockNumber, B256};
use core::ops::RangeBounds;
use reth_storage_errors::provider::ProviderResult;

/// Type of log value for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogValueType {
    /// Log address
    Address,
    /// Log topic with index (0-3)
    Topic(usize),
}

/// Filter criteria for querying logs
#[derive(Debug, Clone, Default)]
pub struct FilterCriteria {
    /// Addresses to filter (empty = any address)
    pub addresses: Vec<Address>,
    /// Topics to filter [topic0, topic1, topic2, topic3]
    /// None = any topic, Some(vec![]) = match none, Some(vec![hash, ...]) = match any of these
    pub topics: [Option<Vec<B256>>; 4],
}

/// Information about a log's position and its value index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFilterInfo {
    /// Global log value index
    pub log_value_index: u64,
    /// Block number where the log occurred
    pub block_number: BlockNumber,
    /// Transaction index within the block
    pub tx_index: u32,
    /// Log index within the transaction
    pub log_index: u32,
}

/// Read-only access to filter map data.
pub trait FilterMapReader: Send + Sync {
    /// Get a filter map row by its global row index.
    ///
    /// Returns the compressed bitmap data if the row exists.
    fn get_filter_map_row(&self, global_row_index: u64) -> ProviderResult<Option<Vec<u8>>>;

    /// Get the log value range for a specific block.
    fn get_block_log_range(&self, block_number: BlockNumber) -> ProviderResult<Option<(u64, u64)>>;

    /// Get map boundary information.
    fn get_map_boundary(&self, map_index: u32) -> ProviderResult<Option<(BlockNumber, u64)>>;

    /// Get filter map metadata.
    fn get_filter_map_metadata(&self) -> ProviderResult<Option<(BlockNumber, u64, u32)>>;

    /// Check if a block range is indexed in the filter maps.
    fn is_filter_map_indexed(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
    ) -> ProviderResult<bool>;

    /// Get the starting log value index for a specific block.
    /// This is used for efficient binary search when locating logs.
    fn get_block_log_value_pointer(&self, block_number: BlockNumber)
        -> ProviderResult<Option<u64>>;

    /// Get multiple block log value pointers in a range.
    /// Returns pairs of (`block_number`, `starting_log_value_index`).
    fn get_block_log_value_pointers_range(
        &self,
        block_range: impl RangeBounds<BlockNumber>,
    ) -> ProviderResult<Vec<(BlockNumber, u64)>>;
}

/// Write access to filter map data.
pub trait FilterMapWriter: Send + Sync {
    /// Put a filter map row at a global row index.
    ///
    /// # Arguments
    /// * `global_row_index` - The global row index (`map_index` * `MAP_HEIGHT` + `row_index`)
    /// * `row_data` - Compressed row data (bitmap)
    fn put_filter_map_row(&self, global_row_index: u64, row_data: Vec<u8>) -> ProviderResult<()>;

    /// Update map boundary information.
    ///
    /// # Arguments
    /// * `map_index` - The filter map index
    /// * `last_block` - Last block in this map
    /// * `last_log_value_index` - Last log value index in this map
    fn put_map_boundary(
        &self,
        map_index: u32,
        last_block: BlockNumber,
        last_log_value_index: u64,
    ) -> ProviderResult<()>;

    /// Update block to log value mapping.
    ///
    /// # Arguments
    /// * `block_number` - The block number
    /// * `start_index` - First log value index in the block
    /// * `end_index` - Last log value index in the block (inclusive)
    fn put_block_log_range(
        &self,
        block_number: BlockNumber,
        start_index: u64,
        end_index: u64,
    ) -> ProviderResult<()>;

    /// Store the starting log value index for a block.
    /// This pointer is used for efficient binary search during queries.
    fn put_block_log_value_pointer(
        &self,
        block_number: BlockNumber,
        start_index: u64,
    ) -> ProviderResult<()>;

    /// Update the global filter map metadata.
    ///
    /// # Arguments
    /// * `indexed_height` - Highest indexed block
    /// * `total_log_values` - Total number of log values indexed
    /// * `total_maps` - Total number of filter maps
    fn put_filter_map_metadata(
        &self,
        indexed_height: BlockNumber,
        total_log_values: u64,
        total_maps: u32,
    ) -> ProviderResult<()>;

    /// Delete filter maps within a block range.
    ///
    /// Returns the number of rows deleted.
    fn delete_filter_maps(&self, block_range: impl RangeBounds<BlockNumber>)
        -> ProviderResult<u64>;
}
