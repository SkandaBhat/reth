//! FilterMaps query engine for efficient log retrieval.

mod builder;
mod constants;
mod matcher;
mod params;
mod processor;
mod query;
mod types;
// mod renderer;
mod utils;

#[cfg(test)]
mod any_test;

pub use builder::{FilterMapBuilder, LogValueIterator, RenderedMap};
pub use constants::{DEFAULT_PARAMS, EXPECTED_MATCHES, RANGE_TEST_PARAMS};
pub use matcher::{FilterMapProvider, Matcher};
pub use params::FilterMapParams;
pub use processor::FilterMapsProcessor;
pub use query::{query_logs, verify_log_matches_filter};
pub use types::{
    FilterError, FilterResult, LogFilter, MatchOrderStats, MatcherResult, PotentialMatches,
    RuntimeStats, RuntimeStatsSummary,
};
// pub use renderer::{MapRenderer, RenderError, RenderStats};
pub use utils::{address_value, topic_value};

/// A full or partial in-memory representation of a filter map where rows are
/// allowed to have a nil value meaning the row is not stored in the structure.
/// Note that therefore a known empty row should be represented with a zero-
/// length slice. It can be used as a memory cache or an overlay while preparing
/// a batch of changes to the structure. In either case a nil value should be
/// interpreted as transparent (uncached/unchanged).
pub type FilterMap = Vec<FilterRow>;

/// FilterRow encodes a single row of a filter map as a list of column indices.
/// Note that the values are always stored in the same order as they were added
/// and if the same column index is added twice, it is also stored twice.
/// Order of column indices and potential duplications do not matter when
/// searching for a value but leaving the original order makes reverting to a
/// previous state simpler.
pub type FilterRow = Vec<u32>;

/// FilterMaps is a collection of filter maps.
#[derive(Debug, Clone)]
pub struct FilterMaps {
    /// The parameters used to create the filter maps.
    pub params: FilterMapParams,
    /// The filter maps.
    pub filter_maps: Vec<FilterMap>,
}
