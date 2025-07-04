//! FilterMaps query engine for efficient log retrieval.

mod mapping;
mod matcher;
mod query;
mod renderer;
mod utils;

pub use log_value::{address_value, process_log, topic_value, LogValue};
pub use mapping::{
    column_index, map_index_from_lv_index, map_row_index, masked_map_index, row_index,
    FilterMapParams,
};
pub use matcher::{FilterMatcher, MatcherResult, SingleMatcher};
pub use query::{FilterMapsError, FilterMapsQueryEngine};
pub use renderer::{MapRenderer, RenderError, RenderStats};
