//! Core types used by the `FilterMaps` implementation.

use alloy_primitives::{BlockNumber, B256};

/// Metadata for a block delimiter in the log value sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockDelimiter {
    /// The parent block hash.
    pub block_hash: B256,
    /// The block number (previous block).
    pub block_number: BlockNumber,
    /// The parent block timestamp.
    pub timestamp: u64,
}

/// Metadata for an actual log value (address or topic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogValue {
    /// The value hash (address or topic).
    pub value: B256,
    /// The transaction hash.
    pub transaction_hash: B256,
    /// The block number.
    pub block_number: BlockNumber,
    /// The transaction index in the block.
    pub transaction_index: u64,
    /// The log index within the transaction.
    pub log_in_tx_index: u64,
}
/// Errors that can occur when using `FilterMaps`.
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    /// The filter matches all logs, which is not supported by `FilterMaps`.
    /// Use legacy filtering for this case.
    #[error("filter matches all logs")]
    MatchAll,

    /// Database error occurred.
    #[error("database error: {0}")]
    Database(String),

    /// Invalid block range specified.
    #[error("invalid block range: {0} > {1}")]
    InvalidRange(u64, u64),

    /// Insufficient layers in filter map row alternatives.
    #[error("insufficient filter map layers for map {0}")]
    InsufficientLayers(u64),

    /// Corrupted filter map data detected.
    #[error("corrupted filter map data: {0}")]
    CorruptedData(String),

    /// Maximum layer limit exceeded.
    #[error("maximum layer limit ({0}) exceeded")]
    MaxLayersExceeded(u64),

    /// Invalid filter map parameters.
    #[error("invalid filter map parameters: {0}")]
    InvalidParameters(String),

    /// Invalid block sequence.
    #[error("invalid block sequence: expected {expected}, got {actual}")]
    InvalidBlockSequence {
        /// The expected block number.
        expected: u64,
        /// The actual block number received.
        actual: u64,
    },

    /// Provider error occurred.
    #[error("provider error: {0}")]
    Provider(String),
}

/// Result type for `FilterMaps` operations.
pub type FilterResult<T> = Result<T, FilterError>;

impl From<reth_errors::ProviderError> for FilterError {
    fn from(err: reth_errors::ProviderError) -> Self {
        Self::Provider(err.to_string())
    }
}

/// A list of potential matching log value indices.
///
/// `None` represents a wildcard match (matches all indices in the map range).
/// An empty `Vec` means no matches were found.
pub type PotentialMatches = Option<Vec<u64>>;

/// Result from a matcher containing matches for a specific map index.
#[derive(Debug, Clone)]
pub struct MatcherResult {
    /// The map index this result is for
    pub map_index: u64,
    /// The potential matches found for this map
    /// None = wildcard (matches all), Some(vec) = specific matches
    pub matches: PotentialMatches,
}
