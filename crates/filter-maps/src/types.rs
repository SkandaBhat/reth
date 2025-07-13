//! Core types used by the `FilterMaps` implementation.

use alloy_primitives::{Address, B256};
use std::sync::atomic::AtomicU64;

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

/// Statistics for adaptive matcher evaluation ordering.
#[derive(Debug, Default)]
pub struct MatchOrderStats {
    /// Total number of evaluations
    pub total_count: AtomicU64,
    /// Number of evaluations that returned non-empty results
    pub non_empty_count: AtomicU64,
    /// Total cost of evaluations (sum of layer indices + 1)
    pub total_cost: AtomicU64,
}

impl MatchOrderStats {
    /// Updates statistics after an evaluation.
    pub fn add(&self, empty: bool, layer_index: u32) {
        use std::sync::atomic::Ordering::Relaxed;

        // Track all evaluations for accurate statistics
        self.total_count.fetch_add(1, Relaxed);
        if !empty {
            self.non_empty_count.fetch_add(1, Relaxed);
        }
        self.total_cost.fetch_add(u64::from(layer_index + 1), Relaxed);
    }

    /// Merges another set of statistics into this one.
    pub fn merge(&self, other: &Self) {
        use std::sync::atomic::Ordering::Relaxed;

        self.total_count.fetch_add(other.total_count.load(Relaxed), Relaxed);
        self.non_empty_count.fetch_add(other.non_empty_count.load(Relaxed), Relaxed);
        self.total_cost.fetch_add(other.total_cost.load(Relaxed), Relaxed);
    }

    /// Calculates the evaluation score for comparison.
    /// Lower score means this matcher should be evaluated first.
    pub fn score(&self) -> f64 {
        use std::sync::atomic::Ordering::Relaxed;

        let total_count = self.total_count.load(Relaxed);
        if total_count == 0 {
            return f64::MAX;
        }

        let total_cost = self.total_cost.load(Relaxed);
        let non_empty_count = self.non_empty_count.load(Relaxed);

        let avg_cost = total_cost as f64 / total_count as f64;
        let empty_rate = 1.0 - (non_empty_count as f64 / total_count as f64);

        // Balance between cost and empty rate
        avg_cost * (1.0 + empty_rate)
    }
}

/// Runtime statistics for performance monitoring using atomic counters.
#[derive(Debug, Default)]
pub struct RuntimeStats {
    /// Number of first layer fetches
    pub fetch_first_count: AtomicU64,
    /// Time spent fetching first layer (nanoseconds)
    pub fetch_first_duration_ns: AtomicU64,
    /// Rows fetched from first layer
    pub fetch_first_rows: AtomicU64,

    /// Number of additional layer fetches
    pub fetch_more_count: AtomicU64,
    /// Time spent fetching additional layers (nanoseconds)
    pub fetch_more_duration_ns: AtomicU64,
    /// Rows fetched from additional layers
    pub fetch_more_rows: AtomicU64,

    /// Number of process operations
    pub process_count: AtomicU64,
    /// Time spent processing matches (nanoseconds)
    pub process_duration_ns: AtomicU64,
    /// Matches found
    pub process_matches: AtomicU64,

    /// Number of logs fetched
    pub get_log_count: AtomicU64,
    /// Time spent fetching logs (nanoseconds)
    pub get_log_duration_ns: AtomicU64,
}

impl RuntimeStats {
    /// Creates a summary of the statistics (non-atomic snapshot).
    pub fn summary(&self) -> RuntimeStatsSummary {
        use std::sync::atomic::Ordering::Relaxed;

        RuntimeStatsSummary {
            fetch_first_count: self.fetch_first_count.load(Relaxed),
            fetch_first_duration_ns: self.fetch_first_duration_ns.load(Relaxed),
            fetch_first_rows: self.fetch_first_rows.load(Relaxed),
            fetch_more_count: self.fetch_more_count.load(Relaxed),
            fetch_more_duration_ns: self.fetch_more_duration_ns.load(Relaxed),
            fetch_more_rows: self.fetch_more_rows.load(Relaxed),
            process_count: self.process_count.load(Relaxed),
            process_duration_ns: self.process_duration_ns.load(Relaxed),
            process_matches: self.process_matches.load(Relaxed),
            get_log_count: self.get_log_count.load(Relaxed),
            get_log_duration_ns: self.get_log_duration_ns.load(Relaxed),
        }
    }
}

/// Non-atomic snapshot of runtime statistics.
#[derive(Debug, Clone)]
pub struct RuntimeStatsSummary {
    /// Number of first layer fetch operations.
    pub fetch_first_count: u64,
    /// Time spent fetching first layers (nanoseconds).
    pub fetch_first_duration_ns: u64,
    /// Rows fetched from first layer.
    pub fetch_first_rows: u64,
    /// Number of additional layer fetch operations.
    pub fetch_more_count: u64,
    /// Time spent fetching additional layers (nanoseconds).
    pub fetch_more_duration_ns: u64,
    /// Rows fetched from additional layers.
    pub fetch_more_rows: u64,
    /// Number of process operations.
    pub process_count: u64,
    /// Time spent processing matches (nanoseconds).
    pub process_duration_ns: u64,
    /// Matches found.
    pub process_matches: u64,
    /// Number of logs fetched.
    pub get_log_count: u64,
    /// Time spent fetching logs (nanoseconds).
    pub get_log_duration_ns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_filter_constraints() {
        let filter = LogFilter::default();
        assert!(filter.matches_all());
        assert!(!filter.has_constraints());

        let filter = LogFilter::new(vec![Address::ZERO], vec![]);
        assert!(!filter.matches_all());
        assert!(filter.has_constraints());

        let filter = LogFilter::new(vec![], vec![vec![B256::ZERO]]);
        assert!(!filter.matches_all());
        assert!(filter.has_constraints());
    }

    #[test]
    fn test_match_order_stats() {
        use std::sync::atomic::Ordering::Relaxed;

        let stats = MatchOrderStats::default();

        // Add some evaluations
        stats.add(false, 0); // Non-empty at layer 0 (cost=1)
        stats.add(true, 0); // Empty at layer 0 (cost=1)
        stats.add(false, 1); // Non-empty at layer 1 (cost=2)
        stats.add(true, 1); // Empty at layer 1 (cost=2)

        assert_eq!(stats.total_count.load(Relaxed), 4);
        assert_eq!(stats.non_empty_count.load(Relaxed), 2);
        assert_eq!(stats.total_cost.load(Relaxed), 6); // 1 + 1 + 2 + 2

        // Test merge
        let other = MatchOrderStats::default();
        other.total_count.store(2, Relaxed);
        other.non_empty_count.store(1, Relaxed);
        other.total_cost.store(3, Relaxed);
        stats.merge(&other);

        assert_eq!(stats.total_count.load(Relaxed), 6);
        assert_eq!(stats.non_empty_count.load(Relaxed), 3);
        assert_eq!(stats.total_cost.load(Relaxed), 9); // 6 + 3
    }
}
