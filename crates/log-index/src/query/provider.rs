//! Provider trait for `FilterMaps` matcher operations.
//!
//! This trait defines the interface for accessing `FilterMaps` data structures
//! and is designed to support both local database access and potential future
//! remote prover implementations for EIP-7745.

use alloy_primitives::Log;
use reth_errors::ProviderResult;

/// Provider interface for accessing log index data structures.
///
/// This trait follows reth's provider pattern and is designed to support
/// both local database implementations and potential future remote prover
/// implementations for EIP-7745.
pub trait FilterMapsQueryProvider: Send + Sync {
    /// Retrieves a log by its log value index.
    ///
    /// Returns None if the log at the given index doesn't exist or has been pruned.
    fn get_log(&self, log_index: u64) -> ProviderResult<Option<Log>>;
}
