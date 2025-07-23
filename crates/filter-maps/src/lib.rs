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
//!
//! ## Usage
//!
//! ```rust
//! use reth_filter_maps::{FilterMapAccumulator, FilterMapParams, extract_log_values_from_block};
//! use reth_filter_maps::storage::{FilterMapsReader, FilterMapsWriter};
//!
//! // Create an accumulator
//! let params = FilterMapParams::default();
//! let mut accumulator = FilterMapAccumulator::new(params, map_index);
//!
//! // Extract log values from block
//! let log_values = extract_log_values_from_block(
//!     log_value_index,
//!     block_number,
//!     block_hash,
//!     parent_hash,
//!     receipts
//! );
//!
//! // Process log values
//! for log_value in log_values {
//!     accumulator.add_log_value(log_value.index, log_value.value)?;
//! }
//! ```

#![doc(
    html_logo_url = "https://raw.githubusercontent.com/paradigmxyz/reth/main/assets/reth-docs.png",
    html_favicon_url = "https://avatars0.githubusercontent.com/u/97369466?s=256",
    issue_tracker_base_url = "https://github.com/paradigmxyz/reth/issues/"
)]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]

mod accumulator;
mod constants;
mod matcher;
mod params;
mod provider;
mod query;
mod state;
pub mod storage;
mod types;
mod utils;

pub use accumulator::FilterMapAccumulator;
pub use constants::{DEFAULT_PARAMS, EXPECTED_MATCHES, MAX_LAYERS, RANGE_TEST_PARAMS};
pub use matcher::Matcher;
pub use params::FilterMapParams;
pub use provider::FilterMapProvider;
pub use query::{address_to_log_value, query_logs, topic_to_log_value, verify_log_matches_filter};
pub use state::FilterMapAccumulatorState;
pub use storage::{FilterMapRow, FilterMapsRange, FilterMapsReader, FilterMapsWriter};
pub use types::{
    FilterError, FilterMapMetadata, FilterResult, LogFilter, LogValue, MatcherResult,
    PotentialMatches,
};
pub use utils::{address_value, extract_log_values_from_block, topic_value};

pub trait FilterMapExt {
    // Convert a filter map to storage rows.
    fn to_storage_rows(&self) -> Vec<(u32, FilterMapRow)>;
}

/// A full or partial in-memory representation of a filter map where rows are
/// allowed to have a nil value meaning the row is not stored in the structure.
/// Note that therefore a known empty row should be represented with a zero-
/// length slice. It can be used as a memory cache or an overlay while preparing
/// a batch of changes to the structure. In either case a nil value should be
/// interpreted as transparent (uncached/unchanged).
pub type FilterMap = Vec<FilterRow>;

impl FilterMapExt for FilterMap {
    fn to_storage_rows(&self) -> Vec<(u32, FilterMapRow)> {
        self.iter()
            .enumerate()
            .filter_map(|(row_idx, columns)| {
                if columns.is_empty() {
                    None
                } else {
                    let cols_u64: Vec<u64> = columns.iter().map(|&c| c as u64).collect();
                    Some((row_idx as u32, FilterMapRow::new(cols_u64)))
                }
            })
            .collect()
    }
}

/// `FilterRow` encodes a single row of a filter map as a list of column indices.
/// Note that the values are always stored in the same order as they were added
/// and if the same column index is added twice, it is also stored twice.
/// Order of column indices and potential duplications do not matter when
/// searching for a value but leaving the original order makes reverting to a
/// previous state simpler.
pub type FilterRow = Vec<u64>;
