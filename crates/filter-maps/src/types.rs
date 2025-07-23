//! Core types used by the `FilterMaps` implementation.

use alloy_primitives::{Address, B256};

#[derive(Debug, Clone)]
pub struct LogValue {
    pub block_number: u64,
    pub block_hash: B256,
    pub index: u64,
    pub value: B256,
    pub is_block_delimiter: bool,
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
    InsufficientLayers(u32),

    /// Corrupted filter map data detected.
    #[error("corrupted filter map data: {0}")]
    CorruptedData(String),

    /// Maximum layer limit exceeded.
    #[error("maximum layer limit ({0}) exceeded")]
    MaxLayersExceeded(u32),

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
    pub map_index: u32,
    /// The potential matches found for this map
    /// None = wildcard (matches all), Some(vec) = specific matches
    pub matches: PotentialMatches,
}

/// Metadata for a completed filter map.
#[derive(Debug, Clone, Default)]
pub struct FilterMapMetadata {
    /// Index of this map
    pub map_index: u32,
    /// First block that has logs in this map
    pub first_block: u64,
    /// Last block that has logs in this map
    pub last_block: u64,
    /// Hash of the last block
    pub last_block_hash: B256,
    /// Log value pointers for blocks starting in this map
    pub block_lv_pointers: Vec<(u64, u64)>,
}

/// Filter criteria for log matching.
#[derive(Debug, Clone, Default)]
pub struct LogFilter {
    /// Addresses to match (empty = match all addresses)
    pub addresses: Vec<Address>,
    /// Topics to match, where each position can have multiple options
    /// (empty at any position = match all for that topic position)
    pub topics: Vec<Vec<B256>>,
}

impl LogFilter {
    /// Creates a new log filter with the given addresses and topics.
    pub const fn new(addresses: Vec<Address>, topics: Vec<Vec<B256>>) -> Self {
        Self { addresses, topics }
    }

    /// Returns true if this filter matches all logs (no constraints).
    pub fn matches_all(&self) -> bool {
        self.addresses.is_empty() && self.topics.is_empty()
    }

    /// Returns true if any position has at least one specific value to match.
    pub fn has_constraints(&self) -> bool {
        !self.addresses.is_empty() || self.topics.iter().any(|t| !t.is_empty())
    }
}
