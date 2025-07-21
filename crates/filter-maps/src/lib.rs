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
//! use reth_filter_maps::{FilterMapsProcessor, FilterMapParams};
//! use reth_filter_maps::storage::{FilterMapsReader, FilterMapsWriter};
//!
//! // Create a processor with storage
//! let params = FilterMapParams::default();
//! let storage = MyStorage::new();
//! let mut processor = FilterMapsProcessor::new(params, storage);
//!
//! // Process blocks
//! processor.process_block(block_number, block_hash, &receipts)?;
//!
//! // Query logs
//! let logs = query_logs(provider, from_block, to_block, addresses, topics)?;
//! ```

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

use alloy_primitives::{BlockNumber, B256};
use alloy_rpc_types_eth::Receipt;
pub use builder::{FilterMapBuilder, LogValueIterator, RenderedMap};
pub use constants::{DEFAULT_PARAMS, EXPECTED_MATCHES, MAX_LAYERS, RANGE_TEST_PARAMS};
pub use matcher::Matcher;
pub use params::FilterMapParams;
pub use processor::FilterMapsProcessor;
pub use provider::FilterMapProvider;
pub use query::{address_to_log_value, query_logs, topic_to_log_value, verify_log_matches_filter};
pub use storage::{FilterMapRow, FilterMapsRange, FilterMapsReader, FilterMapsWriter};
pub use types::{
    FilterError, FilterResult, LogFilter, MatchOrderStats, MatcherResult, PotentialMatches,
    RuntimeStats, RuntimeStatsSummary,
};
pub use utils::{address_value, topic_value};

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

/// `FilterMaps` is a collection of filter maps.
#[derive(Debug, Clone)]
pub struct FilterMaps<S>
where
    S: FilterMapsReader + FilterMapsWriter,
{
    /// The parameters used to create the filter maps.
    pub params: FilterMapParams,
    /// The processor.
    pub processor: FilterMapsProcessor<S>,
}

impl<S> FilterMaps<S>
where
    S: FilterMapsReader + FilterMapsWriter,
{
    /// Creates a new filter maps instance.
    pub fn new(params: FilterMapParams, storage: S) -> Self {
        Self {
            params: params.clone(),
            processor: FilterMapsProcessor::new(params.clone(), storage),
        }
    }
}
