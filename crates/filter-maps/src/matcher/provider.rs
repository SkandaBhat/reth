//! Provider trait for `FilterMaps` matcher operations.
//!
//! This trait defines the interface for accessing `FilterMaps` data structures
//! and is designed to support both local database access and potential future
//! remote prover implementations for EIP-7745.

use crate::{params::FilterMapParams, FilterRow};
use alloy_primitives::BlockNumber;
use alloy_rpc_types_eth::Log;
use reth_errors::ProviderResult;

/// Provider interface for accessing log index data structures.
///
/// This trait follows reth's provider pattern and is designed to support
/// both local database implementations and potential future remote prover
/// implementations for EIP-7745.
pub trait FilterMapProvider: Send + Sync {
    /// Returns the filter map parameters used by this provider.
    fn params(&self) -> &FilterMapParams;

    /// Maps a block number to the log value index of its first log.
    ///
    /// Returns an error if the block is not indexed or doesn't exist.
    fn block_to_log_index(&self, block_number: BlockNumber) -> ProviderResult<u64>;

    /// Retrieves filter map rows for the given map indices and row index.
    ///
    /// # Arguments
    /// * `map_indices` - The indices of the maps to fetch rows from
    /// * `row_index` - The row index within each map
    /// * `layer` - The layer to fetch from (0 = base layer)
    ///
    /// Returns a vector of filter rows corresponding to each map index.
    fn get_filter_rows(
        &self,
        map_indices: &[u32],
        row_index: u32,
        layer: u32,
    ) -> ProviderResult<Vec<FilterRow>>;

    /// Retrieves a log by its log value index.
    ///
    /// Returns None if the log at the given index doesn't exist or has been pruned.
    fn get_log(&self, log_index: u64) -> ProviderResult<Option<Log>>;
}
