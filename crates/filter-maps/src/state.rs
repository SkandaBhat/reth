use std::collections::{HashMap, VecDeque};

use alloy_primitives::B256;

use crate::{
    accumulator::{FilterMap, FilterMapAccumulator},
    FilterMapParams,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterMapAccumulatorState {
    /// The filter map parameters
    pub params: FilterMapParams,
    /// Log value index
    pub log_value_index: u64,
    /// Current filter map being built
    pub current_map: FilterMap,
    /// Row fill levels for each layer
    pub row_fill_levels: Vec<HashMap<u64, u64>>,
    /// Cached row calculations
    pub row_cache: HashMap<(u64, B256), u64>,
    /// Completed maps
    pub completed_maps: VecDeque<FilterMap>,
}
// Convert FilterMapBuilder to FilterMapBuilderState (for saving)
impl From<FilterMapAccumulator> for FilterMapAccumulatorState {
    fn from(builder: FilterMapAccumulator) -> Self {
        Self {
            params: builder.params,
            log_value_index: builder.log_value_index,
            current_map: builder.current_map,
            row_fill_levels: builder.row_fill_levels,
            row_cache: builder.row_cache,
            completed_maps: builder.completed_maps,
        }
    }
}

// Convert FilterMapBuilderState back to FilterMapBuilder (for restoring)
impl From<FilterMapAccumulatorState> for FilterMapAccumulator {
    fn from(state: FilterMapAccumulatorState) -> Self {
        Self {
            params: state.params,
            log_value_index: state.log_value_index,
            current_map: state.current_map,
            row_fill_levels: state.row_fill_levels,
            row_cache: state.row_cache,
            completed_maps: state.completed_maps,
        }
    }
}

// todo: compact
