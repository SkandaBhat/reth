use std::collections::HashMap;

use alloy_primitives::B256;

use crate::{accumulator::FilterMapAccumulator, FilterMap, FilterMapParams};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterMapAccumulatorState {
    /// The filter map parameters
    pub params: FilterMapParams,
    /// Current map index
    pub map_index: u32,
    /// Current filter map being built
    pub current_map: FilterMap,
    /// Row fill levels for each layer
    pub row_fill_levels: Vec<HashMap<u32, u32>>,
    /// Cached row calculations
    pub row_cache: HashMap<(u32, B256), u32>,
}
// Convert FilterMapBuilder to FilterMapBuilderState (for saving)
impl From<FilterMapAccumulator> for FilterMapAccumulatorState {
    fn from(builder: FilterMapAccumulator) -> Self {
        Self {
            params: builder.params,
            map_index: builder.map_index,
            current_map: builder.current_map,
            row_fill_levels: builder.row_fill_levels,
            row_cache: builder.row_cache,
        }
    }
}

// Convert FilterMapBuilderState back to FilterMapBuilder (for restoring)
impl From<FilterMapAccumulatorState> for FilterMapAccumulator {
    fn from(state: FilterMapAccumulatorState) -> Self {
        Self {
            params: state.params,
            map_index: state.map_index,
            current_map: state.current_map,
            row_fill_levels: state.row_fill_levels,
            row_cache: state.row_cache,
        }
    }
}

// todo: compact
