//! Filter maps implementation for efficient log indexing (EIP-7745).
//!
//! This crate provides the core implementation of filter maps as specified in EIP-7745,
//! which enables efficient querying of Ethereum logs without relying on bloom filters.
//!
//! ## Overview
//!
//! Filter maps are two-dimensional sparse bit maps that index log values (addresses and topics)
//! for efficient searching. They provide:
//!
//! - Constant-time lookups for log queries
//! - Better storage efficiency compared to bloom filters
//! - Support for range queries across blocks
//! - Multi-layer overflow handling for collision management

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/paradigmxyz/reth/main/assets/reth-docs.png",
    html_favicon_url = "https://avatars0.githubusercontent.com/u/97369466?s=256",
    issue_tracker_base_url = "https://github.com/paradigmxyz/reth/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

mod builder;
mod constants;
mod matcher;
mod params;
mod processor;
mod provider;
mod query;
pub mod storage;
mod types;
mod utils;

#[cfg(any(test, feature = "test-utils", doctest))]
pub mod test_utils;

pub use builder::{FilterMapBuilder, LogValueIterator, RenderedMap};
pub use constants::{DEFAULT_PARAMS, EXPECTED_MATCHES, MAX_LAYERS, RANGE_TEST_PARAMS};
pub use matcher::Matcher;
pub use params::FilterMapParams;
pub use processor::FilterMapsProcessor;
pub use provider::FilterMapProvider;
pub use query::{address_to_log_value, query_logs, topic_to_log_value, verify_log_matches_filter};
pub use types::{
    FilterError, FilterResult, LogFilter, MatchOrderStats, MatcherResult, PotentialMatches,
    RuntimeStats, RuntimeStatsSummary,
};
pub use utils::{address_value, topic_value};

/// A full or partial in-memory representation of a filter map where rows are
/// allowed to have a nil value meaning the row is not stored in the structure.
/// Note that therefore a known empty row should be represented with a zero-
/// length slice. It can be used as a memory cache or an overlay while preparing
/// a batch of changes to the structure. In either case a nil value should be
/// interpreted as transparent (uncached/unchanged).
pub type FilterMap = Vec<FilterRow>;

/// `FilterRow` encodes a single row of a filter map as a list of column indices.
/// Note that the values are always stored in the same order as they were added
/// and if the same column index is added twice, it is also stored twice.
/// Order of column indices and potential duplications do not matter when
/// searching for a value but leaving the original order makes reverting to a
/// previous state simpler.
pub type FilterRow = Vec<u64>;

/// `FilterMaps` is a collection of filter maps.
#[derive(Debug, Clone)]
pub struct FilterMaps {
    /// The parameters used to create the filter maps.
    pub params: FilterMapParams,
    /// The filter maps.
    pub filter_maps: Vec<FilterMap>,
}
