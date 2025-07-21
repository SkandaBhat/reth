//! Matcher hierarchy for efficient log filtering.
//!
//! This module implements a hierarchical matcher system that can efficiently
//! search through `FilterMaps` to find logs matching given criteria.

use crate::{
    params::FilterMapParams,
    types::{FilterResult, MatchOrderStats, MatcherResult, PotentialMatches, RuntimeStats},
    FilterRow,
};
use alloy_primitives::B256;
use std::{
    collections::{HashMap, HashSet},
    sync::{atomic::Ordering, Arc},
    time::Instant,
};

use crate::provider::FilterMapProvider;

/// A matcher that can search for logs matching specific criteria.
///
/// This enum represents different types of matchers in a sum type pattern,
/// which is more idiomatic in Rust than the trait/instance pattern.
#[derive(Clone)]
pub enum Matcher {
    /// Matches a single log value hash.
    Single {
        /// The provider for accessing filter map data.
        provider: Arc<dyn FilterMapProvider>,
        /// The value to match.
        value: B256,
        /// Runtime statistics for this matcher.
        stats: Arc<RuntimeStats>,
    },
    /// Matches any of the child matchers (OR operation).
    Any {
        /// The child matchers.
        matchers: Vec<Matcher>,
    },
    /// Matches a sequence of values at consecutive log indices.
    Sequence {
        /// Filter map parameters.
        params: Arc<FilterMapParams>,
        /// Base matcher (matches at index X).
        base: Box<Matcher>,
        /// Next matcher (matches at index X + offset).
        next: Box<Matcher>,
        /// Offset between base and next.
        offset: u64,
        /// Statistics for optimizing evaluation order.
        base_stats: Arc<MatchOrderStats>,
        /// Statistics for optimizing evaluation order for the next matcher.
        next_stats: Arc<MatchOrderStats>,
    },
}

impl std::fmt::Debug for Matcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Single { value, stats, .. } => {
                f.debug_struct("Single").field("value", value).field("stats", stats).finish()
            }
            Self::Any { matchers } => f.debug_struct("Any").field("matchers", matchers).finish(),
            Self::Sequence { params, base, next, offset, base_stats, next_stats } => f
                .debug_struct("Sequence")
                .field("params", params)
                .field("base", base)
                .field("next", next)
                .field("offset", offset)
                .field("base_stats", base_stats)
                .field("next_stats", next_stats)
                .finish(),
        }
    }
}

impl Matcher {
    /// Creates a new single value matcher.
    pub fn single(provider: Arc<dyn FilterMapProvider>, value: B256) -> Self {
        Self::Single { provider, value, stats: Arc::new(RuntimeStats::default()) }
    }

    /// Creates a new ANY matcher that matches if any child matches.
    pub const fn any(matchers: Vec<Self>) -> Self {
        Self::Any { matchers }
    }

    /// Creates a new sequence matcher.
    pub fn sequence(
        params: Arc<FilterMapParams>,
        base: Box<Self>,
        next: Box<Self>,
        offset: u64,
    ) -> Self {
        Self::Sequence {
            params,
            base,
            next,
            offset,
            base_stats: Arc::new(MatchOrderStats::default()),
            next_stats: Arc::new(MatchOrderStats::default()),
        }
    }

    /// Creates a sequence matcher from a list of matchers.
    ///
    /// The resulting matcher signals a match at log value index X when each
    /// underlying matcher matchers[i] returns a match at X+i.
    pub fn sequence_from_slice(params: Arc<FilterMapParams>, mut matchers: Vec<Self>) -> Self {
        match matchers.len() {
            0 => panic!("Empty sequence matchers not allowed"),
            1 => matchers.into_iter().next().unwrap(),
            _ => {
                // Build sequence like Go: take last as next, rest as base
                // For matchers [A, B, C], we want:
                // Sequence(base: Sequence(base: A, next: B, offset: 1), next: C, offset: 2)
                let len = matchers.len();
                let next = matchers.pop().unwrap();
                let base = Self::sequence_from_slice(params.clone(), matchers);

                Self::sequence(params, Box::new(base), Box::new(next), (len - 1) as u64)
            }
        }
    }

    /// Processes the matcher for the given map indices and returns all matches.
    pub fn process(&self, map_indices: &[u32]) -> FilterResult<Vec<MatcherResult>> {
        let mut state = MatcherState::new(self, map_indices);
        let mut all_results = Vec::new();

        // Process layers until complete
        let mut layer = 0;
        while !state.is_complete() {
            // Check layer limit to prevent infinite loops
            if layer >= crate::constants::MAX_LAYERS {
                return Err(crate::types::FilterError::MaxLayersExceeded(
                    crate::constants::MAX_LAYERS,
                ));
            }

            let layer_results = state.process_layer(layer)?;
            all_results.extend(layer_results);
            layer += 1;
        }

        Ok(all_results)
    }
}

/// State for processing a matcher.
struct MatcherState {
    /// The matcher being processed.
    matcher: Matcher,
    /// Variant-specific state.
    state: MatcherVariantState,
}

/// Variant-specific state for matcher processing.
enum MatcherVariantState {
    /// State for single matcher.
    Single {
        /// Active map indices still being processed.
        active_indices: Vec<u32>,
        /// Accumulated filter rows for each map.
        filter_rows: HashMap<u32, Vec<FilterRow>>,
    },
    /// State for ANY matcher.
    Any {
        /// States for child matchers.
        child_states: Vec<MatcherState>,
        /// Results being accumulated for each map.
        child_results: HashMap<u32, MatchAnyResults>,
    },
    /// State for sequence matcher.
    Sequence {
        /// State for base matcher.
        base_state: Box<MatcherState>,
        /// State for next matcher.
        next_state: Box<MatcherState>,
        /// Maps we still need results for.
        need_matched: HashSet<u32>,
        /// Maps requested from base.
        base_requested: HashSet<u32>,
        /// Maps requested from next.
        next_requested: HashSet<u32>,
        /// Results from base matcher.
        base_results: HashMap<u32, PotentialMatches>,
        /// Results from next matcher.
        next_results: HashMap<u32, PotentialMatches>,
    },
}

/// Results being collected for ANY matcher.
#[derive(Clone)]
struct MatchAnyResults {
    /// Matches from each child.
    matches: Vec<PotentialMatches>,
    /// Which children have completed.
    done: Vec<bool>,
    /// Number of children still needed.
    need_more: usize,
}

impl MatcherState {
    /// Creates a new matcher state for the given indices.
    fn new(matcher: &Matcher, map_indices: &[u32]) -> Self {
        let state = match matcher {
            Matcher::Single { .. } => {
                let mut filter_rows = HashMap::with_capacity(map_indices.len());
                for &idx in map_indices {
                    filter_rows.insert(idx, Vec::new());
                }
                MatcherVariantState::Single { active_indices: map_indices.to_vec(), filter_rows }
            }
            Matcher::Any { matchers } => {
                let child_states = matchers.iter().map(|m| Self::new(m, map_indices)).collect();

                let mut child_results = HashMap::with_capacity(map_indices.len());
                for &idx in map_indices {
                    child_results.insert(
                        idx,
                        MatchAnyResults {
                            matches: vec![None; matchers.len()],
                            done: vec![false; matchers.len()],
                            need_more: matchers.len(),
                        },
                    );
                }

                MatcherVariantState::Any { child_states, child_results }
            }
            Matcher::Sequence { base, next, .. } => {
                let base_state = Box::new(Self::new(base, map_indices));
                let next_state = Box::new(Self::new(next, map_indices));

                let mut need_matched = HashSet::with_capacity(map_indices.len());
                let mut base_requested = HashSet::with_capacity(map_indices.len());
                let mut next_requested = HashSet::with_capacity(map_indices.len());

                for &idx in map_indices {
                    need_matched.insert(idx);
                    base_requested.insert(idx);
                    next_requested.insert(idx);
                }

                MatcherVariantState::Sequence {
                    base_state,
                    next_state,
                    need_matched,
                    base_requested,
                    next_requested,
                    base_results: HashMap::new(),
                    next_results: HashMap::new(),
                }
            }
        };

        Self { matcher: matcher.clone(), state }
    }

    /// Processes a single layer and returns any completed results.
    fn process_layer(&mut self, layer: u32) -> FilterResult<Vec<MatcherResult>> {
        match &mut self.state {
            MatcherVariantState::Single { active_indices, filter_rows } => match &self.matcher {
                Matcher::Single { provider, value, stats } => Self::process_single_layer_static(
                    provider,
                    value,
                    stats,
                    layer,
                    active_indices,
                    filter_rows,
                ),
                _ => unreachable!(),
            },
            MatcherVariantState::Any { child_states, child_results } => {
                Self::process_any_layer_static(layer, child_states, child_results)
            }
            MatcherVariantState::Sequence {
                base_state,
                next_state,
                need_matched,
                base_requested,
                next_requested,
                base_results,
                next_results,
            } => match &self.matcher {
                Matcher::Sequence { params, offset, base_stats, next_stats, .. } => {
                    Self::process_sequence_layer_static(
                        params,
                        *offset,
                        base_stats,
                        next_stats,
                        layer,
                        base_state,
                        next_state,
                        need_matched,
                        base_requested,
                        next_requested,
                        base_results,
                        next_results,
                    )
                }
                _ => unreachable!(),
            },
        }
    }

    /// Checks if processing is complete.
    fn is_complete(&self) -> bool {
        match &self.state {
            MatcherVariantState::Single { active_indices, .. } => active_indices.is_empty(),
            MatcherVariantState::Any { child_results, .. } => child_results.is_empty(),
            MatcherVariantState::Sequence { need_matched, .. } => need_matched.is_empty(),
        }
    }

    /// Process layer for single matcher.
    fn process_single_layer_static(
        provider: &Arc<dyn FilterMapProvider>,
        value: &B256,
        stats: &Arc<RuntimeStats>,
        layer: u32,
        active_indices: &mut Vec<u32>,
        filter_rows: &mut HashMap<u32, Vec<FilterRow>>,
    ) -> FilterResult<Vec<MatcherResult>> {
        let params = provider.params();
        let mut results = Vec::new();
        let mut ptr = 0;

        while ptr < active_indices.len() {
            // Find next group of map indices mapped onto the same row
            let map_index = active_indices[ptr];
            let masked_map_index = params.masked_map_index(map_index, layer);
            let row_index = params.row_index(map_index, layer, value);

            // Collect all indices in this group
            let group_start = ptr;
            let mut group_end = ptr + 1;
            while group_end < active_indices.len() {
                let next_idx = active_indices[group_end];
                if params.masked_map_index(next_idx, layer) != masked_map_index {
                    break;
                }
                group_end += 1;
            }

            // Fetch rows for the group
            let fetch_start = Instant::now();
            let group_indices = &active_indices[group_start..group_end];
            let group_rows = provider.get_filter_rows(group_indices, row_index, layer)?;
            let fetch_duration = fetch_start.elapsed();

            // Update statistics
            if layer == 0 {
                stats.fetch_first_count.fetch_add(1, Ordering::Relaxed);
                stats
                    .fetch_first_duration_ns
                    .fetch_add(fetch_duration.as_nanos() as u64, Ordering::Relaxed);
            } else {
                stats.fetch_more_count.fetch_add(1, Ordering::Relaxed);
                stats
                    .fetch_more_duration_ns
                    .fetch_add(fetch_duration.as_nanos() as u64, Ordering::Relaxed);
            }

            // Process each map in the group
            for (i, &map_index) in group_indices.iter().enumerate() {
                let filter_row = &group_rows[i];

                // Get existing filter rows for this map
                let rows =
                    filter_rows.get_mut(&map_index).expect("Map index should exist in filter_rows");

                // Update statistics
                if layer == 0 {
                    stats.fetch_first_rows.fetch_add(filter_row.len() as u64, Ordering::Relaxed);
                } else {
                    stats.fetch_more_rows.fetch_add(filter_row.len() as u64, Ordering::Relaxed);
                }

                // Add the row
                rows.push(filter_row.clone());

                // Check if we've reached the maximum row length for this layer
                if (filter_row.len() as u32) < params.max_row_length(layer) {
                    // Process matches
                    let process_start = Instant::now();
                    let matches = params.potential_matches(rows, map_index, value)?;
                    let process_duration = process_start.elapsed();

                    // Update statistics
                    stats.process_count.fetch_add(1, Ordering::Relaxed);
                    stats
                        .process_duration_ns
                        .fetch_add(process_duration.as_nanos() as u64, Ordering::Relaxed);
                    stats.process_matches.fetch_add(matches.len() as u64, Ordering::Relaxed);

                    results.push(MatcherResult { map_index, matches: Some(matches) });

                    // Remove from tracking since we have a result
                    filter_rows.remove(&map_index);
                }
            }

            ptr = group_end;
        }

        // Clean up active_indices to remove completed maps
        active_indices.retain(|&idx| filter_rows.contains_key(&idx));

        Ok(results)
    }

    /// Process layer for ANY matcher.
    fn process_any_layer_static(
        layer: u32,
        child_states: &mut [Self],
        child_results: &mut HashMap<u32, MatchAnyResults>,
    ) -> FilterResult<Vec<MatcherResult>> {
        let mut merged_results = Vec::new();

        // Special case: empty ANY matcher is a wildcard
        if child_states.is_empty() {
            for &map_index in child_results.keys().collect::<Vec<_>>() {
                merged_results.push(MatcherResult {
                    map_index,
                    matches: None, // Wildcard
                });
            }
            child_results.clear();
            return Ok(merged_results);
        }

        // Process each child
        for (i, child_state) in child_states.iter_mut().enumerate() {
            let results = child_state.process_layer(layer)?;

            for result in results {
                if let Some(mr) = child_results.get_mut(&result.map_index) {
                    if !mr.done[i] {
                        mr.done[i] = true;
                        let matches_is_none = result.matches.is_none();
                        mr.matches[i] = result.matches;
                        mr.need_more -= 1;

                        // If we have all results or found a wildcard, merge
                        if mr.need_more == 0 || matches_is_none {
                            merged_results.push(MatcherResult {
                                map_index: result.map_index,
                                matches: merge_potential_matches(&mr.matches),
                            });
                            child_results.remove(&result.map_index);
                        }
                    }
                }
            }
        }

        Ok(merged_results)
    }

    /// Process layer for sequence matcher.
    #[allow(clippy::too_many_arguments)]
    fn process_sequence_layer_static(
        params: &Arc<FilterMapParams>,
        offset: u64,
        base_stats: &Arc<MatchOrderStats>,
        next_stats: &Arc<MatchOrderStats>,
        layer: u32,
        base_state: &mut Self,
        next_state: &mut Self,
        need_matched: &mut HashSet<u32>,
        base_requested: &mut HashSet<u32>,
        next_requested: &mut HashSet<u32>,
        base_results: &mut HashMap<u32, PotentialMatches>,
        next_results: &mut HashMap<u32, PotentialMatches>,
    ) -> FilterResult<Vec<MatcherResult>> {
        // Determine evaluation order
        let base_first = should_eval_base_first(base_stats, next_stats);

        // Process in optimal order
        if base_first {
            Self::eval_base_static(layer, base_state, base_results, base_requested, base_stats)?;
            Self::drop_next_if_needed_static(
                base_results,
                need_matched,
                next_requested,
                next_state,
            );

            Self::eval_next_static(layer, next_state, next_results, next_requested, next_stats)?;
            Self::drop_base_if_needed_static(
                next_results,
                need_matched,
                base_requested,
                base_state,
            );
        } else {
            Self::eval_next_static(layer, next_state, next_results, next_requested, next_stats)?;
            Self::drop_base_if_needed_static(
                next_results,
                need_matched,
                base_requested,
                base_state,
            );

            Self::eval_base_static(layer, base_state, base_results, base_requested, base_stats)?;
            Self::drop_next_if_needed_static(
                base_results,
                need_matched,
                next_requested,
                next_state,
            );
        }

        // Collect completed results
        let mut matched_results = Vec::new();
        let completed_indices: Vec<_> = need_matched
            .iter()
            .filter(|&&idx| !base_requested.contains(&idx) && !next_requested.contains(&idx))
            .copied()
            .collect();

        for map_index in completed_indices {
            let base_matches = base_results.get(&map_index).cloned().flatten();
            let next_matches = next_results.get(&map_index).cloned().flatten();

            matched_results.push(MatcherResult {
                map_index,
                matches: intersect_with_offset(
                    &base_matches,
                    &next_matches,
                    offset,
                    map_index,
                    params.log_values_per_map as u64,
                ),
            });

            need_matched.remove(&map_index);
        }

        Ok(matched_results)
    }

    /// Evaluate base matcher and update results.
    fn eval_base_static(
        layer: u32,
        base_state: &mut Self,
        base_results: &mut HashMap<u32, PotentialMatches>,
        base_requested: &mut HashSet<u32>,
        base_stats: &Arc<MatchOrderStats>,
    ) -> FilterResult<()> {
        let results = base_state.process_layer(layer)?;

        for result in results {
            let is_empty = result.matches.as_ref().map(|m| m.is_empty()).unwrap_or(false);
            base_results.insert(result.map_index, result.matches);
            base_requested.remove(&result.map_index);

            // Update statistics
            update_match_order_stats(base_stats, is_empty, layer);
        }

        Ok(())
    }

    /// Evaluate next matcher and update results.
    fn eval_next_static(
        layer: u32,
        next_state: &mut Self,
        next_results: &mut HashMap<u32, PotentialMatches>,
        next_requested: &mut HashSet<u32>,
        next_stats: &Arc<MatchOrderStats>,
    ) -> FilterResult<()> {
        let results = next_state.process_layer(layer)?;

        for result in results {
            let is_empty = result.matches.as_ref().map(|m| m.is_empty()).unwrap_or(false);
            next_results.insert(result.map_index, result.matches);
            next_requested.remove(&result.map_index);

            // Update statistics
            update_match_order_stats(next_stats, is_empty, layer);
        }

        Ok(())
    }

    /// Drop indices from next matcher if base results allow it.
    fn drop_next_if_needed_static(
        base_results: &HashMap<u32, PotentialMatches>,
        need_matched: &HashSet<u32>,
        next_requested: &mut HashSet<u32>,
        next_state: &mut Self,
    ) {
        let mut drop_indices = Vec::new();

        for &map_index in next_requested.iter() {
            if let Some(base) = base_results.get(&map_index) {
                // Can drop if base is empty and we need a match
                if need_matched.contains(&map_index)
                    && base.as_ref().map(|m| m.is_empty()).unwrap_or(false)
                {
                    drop_indices.push(map_index);
                }
            }
        }

        for idx in &drop_indices {
            next_requested.remove(idx);
        }

        if !drop_indices.is_empty() {
            next_state.drop_indices(&drop_indices);
        }
    }

    /// Drop indices from base matcher if next results allow it.
    fn drop_base_if_needed_static(
        next_results: &HashMap<u32, PotentialMatches>,
        need_matched: &HashSet<u32>,
        base_requested: &mut HashSet<u32>,
        base_state: &mut Self,
    ) {
        let mut drop_indices = Vec::new();

        for &map_index in base_requested.iter() {
            if let Some(next) = next_results.get(&map_index) {
                // Can drop if next is empty and we need a match
                if need_matched.contains(&map_index)
                    && next.as_ref().map(|m| m.is_empty()).unwrap_or(false)
                {
                    drop_indices.push(map_index);
                }
            }
        }

        for idx in &drop_indices {
            base_requested.remove(idx);
        }

        if !drop_indices.is_empty() {
            base_state.drop_indices(&drop_indices);
        }
    }

    /// Drop the specified indices from further processing.
    fn drop_indices(&mut self, indices: &[u32]) {
        match &mut self.state {
            MatcherVariantState::Single { active_indices, filter_rows } => {
                for &idx in indices {
                    filter_rows.remove(&idx);
                }
                active_indices.retain(|&idx| filter_rows.contains_key(&idx));
            }
            MatcherVariantState::Any { child_states, child_results } => {
                for child in child_states {
                    child.drop_indices(indices);
                }
                for &idx in indices {
                    child_results.remove(&idx);
                }
            }
            MatcherVariantState::Sequence { base_state, next_state, need_matched, .. } => {
                base_state.drop_indices(indices);
                next_state.drop_indices(indices);
                for &idx in indices {
                    need_matched.remove(&idx);
                }
            }
        }
    }
}

/// Helper function to merge multiple lists of potential matches.
///
/// For ANY matcher: Returns the union of all matches, sorted and deduplicated.
/// Returns None if any input is None (wildcard).
/// Returns empty vec only if all inputs are empty or there are no inputs.
pub(crate) fn merge_potential_matches(results: &[PotentialMatches]) -> PotentialMatches {
    if results.is_empty() {
        return Some(vec![]);
    }

    // Check for wildcards (None)
    if results.iter().any(|r| r.is_none()) {
        return None;
    }

    // Collect all non-None results
    let all_some: Vec<_> = results.iter().filter_map(|r| r.as_ref()).collect();

    // If all results are empty, return empty
    if all_some.iter().all(|r| r.is_empty()) {
        return Some(vec![]);
    }

    // Merge all results
    let total_len: usize = all_some.iter().map(|r| r.len()).sum();
    let mut merged = Vec::with_capacity(total_len);

    // Use a min-heap approach to merge sorted lists
    let mut indices = vec![0usize; all_some.len()];

    loop {
        let mut best_idx = None;
        let mut best_val = u64::MAX;

        for (i, results) in all_some.iter().enumerate() {
            if indices[i] < results.len() && results[indices[i]] < best_val {
                best_idx = Some(i);
                best_val = results[indices[i]];
            }
        }

        match best_idx {
            Some(idx) => {
                // Add to merged if not duplicate
                if merged.is_empty() || merged.last() != Some(&best_val) {
                    merged.push(best_val);
                }
                indices[idx] += 1;
            }
            None => break, // All lists exhausted
        }
    }

    Some(merged)
}

/// Helper function to intersect two lists of potential matches for sequence matching.
///
/// Used when matching sequences where base matches at X and next matches at X+offset.
pub(crate) fn intersect_with_offset(
    base: &PotentialMatches,
    next: &PotentialMatches,
    offset: u64,
    map_index: u32,
    values_per_map: u64,
) -> PotentialMatches {
    match (base, next) {
        (None, None) => None, // Both wildcards
        (None, Some(next_matches)) => {
            // Base is wildcard, filter next matches
            let min = (u64::from(map_index) << values_per_map) + offset;
            #[allow(clippy::filter_map_bool_then)]
            let filtered: Vec<_> =
                next_matches.iter().filter_map(|&v| (v >= min).then(|| v - offset)).collect();
            Some(filtered)
        }
        (Some(base_matches), None) => {
            // Next is wildcard, return base matches
            Some(base_matches.clone())
        }
        (Some(base_matches), Some(next_matches)) => {
            // Both have specific matches, find intersection
            if base_matches.is_empty() || next_matches.is_empty() {
                return Some(vec![]);
            }

            let mut result = Vec::with_capacity(base_matches.len().min(next_matches.len()));
            let mut base_iter = base_matches.iter();
            let mut next_iter = next_matches.iter();

            let mut base_val = base_iter.next();
            let mut next_val = next_iter.next();

            while let (Some(&b), Some(&n)) = (base_val, next_val) {
                let b_plus_offset = b + offset;
                match b_plus_offset.cmp(&n) {
                    std::cmp::Ordering::Less => base_val = base_iter.next(),
                    std::cmp::Ordering::Greater => next_val = next_iter.next(),
                    std::cmp::Ordering::Equal => {
                        result.push(b);
                        base_val = base_iter.next();
                        next_val = next_iter.next();
                    }
                }
            }

            Some(result)
        }
    }
}

/// Determines if base should be evaluated before next based on statistics.
fn should_eval_base_first(
    base_stats: &Arc<MatchOrderStats>,
    next_stats: &Arc<MatchOrderStats>,
) -> bool {
    let base_total_cost = base_stats.total_cost.load(Ordering::Relaxed) as f64;
    let base_total_count = base_stats.total_count.load(Ordering::Relaxed) as f64;
    let base_non_empty = base_stats.non_empty_count.load(Ordering::Relaxed) as f64;

    let next_total_cost = next_stats.total_cost.load(Ordering::Relaxed) as f64;
    let next_total_count = next_stats.total_count.load(Ordering::Relaxed) as f64;
    let next_non_empty = next_stats.non_empty_count.load(Ordering::Relaxed) as f64;

    // Evaluate base first if it's expected to be cheaper overall
    base_total_cost.mul_add(next_total_count, base_non_empty * next_total_cost)
        < base_total_cost.mul_add(next_non_empty, next_total_cost * base_total_count)
}

/// Updates match order statistics after evaluating a matcher.
fn update_match_order_stats(stats: &Arc<MatchOrderStats>, is_empty: bool, layer: u32) {
    // Always update statistics for accurate tracking
    stats.total_count.fetch_add(1, Ordering::Relaxed);
    if !is_empty {
        stats.non_empty_count.fetch_add(1, Ordering::Relaxed);
    }
    stats.total_cost.fetch_add((layer + 1) as u64, Ordering::Relaxed);
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_max_layers_limit() {
//         use crate::{FilterMapParams, FilterRow};
//         use alloy_primitives::{b256, BlockNumber};
//         use alloy_rpc_types_eth::Log;
//         use reth_errors::ProviderResult;

//         // Create a provider that always returns full rows
//         struct AlwaysFullProvider {
//             params: FilterMapParams,
//         }

//         impl FilterMapProvider for AlwaysFullProvider {
//             fn params(&self) -> &FilterMapParams {
//                 &self.params
//             }

//             fn block_to_log_index(&self, _: BlockNumber) -> ProviderResult<u64> {
//                 Ok(0)
//             }

//             fn get_filter_rows(
//                 &self,
//                 map_indices: &[u32],
//                 _: u32,
//                 layer: u32,
//             ) -> ProviderResult<Vec<FilterRow>> {
//                 // Always return rows that are exactly at max length
//                 let max_len = self.params.max_row_length(layer) as usize;
//                 let row = vec![0u64; max_len];
//                 Ok(vec![row; map_indices.len()])
//             }

//             fn get_log(&self, _: u64) -> ProviderResult<Option<Log>> {
//                 Ok(None)
//             }
//         }

//         let provider = Arc::new(AlwaysFullProvider { params: crate::constants::DEFAULT_PARAMS });

//         let value = b256!("0000000000000000000000000000000000000000000000000000000000000001");
//         let matcher = Matcher::single(provider, value);

//         // This should hit the MAX_LAYERS limit and return an error
//         let result = matcher.process(&[0]);
//         assert!(matches!(result, Err(crate::types::FilterError::MaxLayersExceeded(_))));
//     }

//     #[test]
//     fn test_merge_potential_matches() {
//         // Empty input
//         assert_eq!(merge_potential_matches(&[]), Some(vec![]));

//         // Single wildcard
//         assert_eq!(merge_potential_matches(&[None]), None);

//         // Mixed with wildcard
//         assert_eq!(merge_potential_matches(&[Some(vec![1, 2]), None]), None);

//         // Empty result mixed with non-empty (should return union)
//         assert_eq!(merge_potential_matches(&[Some(vec![]), Some(vec![1, 2])]), Some(vec![1, 2]));

//         // All empty results
//         assert_eq!(merge_potential_matches(&[Some(vec![]), Some(vec![])]), Some(vec![]));

//         // Normal merge
//         let result = merge_potential_matches(&[
//             Some(vec![1, 3, 5]),
//             Some(vec![2, 3, 6]),
//             Some(vec![3, 4, 7]),
//         ]);
//         assert_eq!(result, Some(vec![1, 2, 3, 4, 5, 6, 7]));
//     }

//     #[test]
//     fn test_intersect_with_offset() {
//         let values_per_map = 16; // 2^16 values per map

//         // Both wildcards
//         assert_eq!(intersect_with_offset(&None, &None, 2, 0, values_per_map), None);

//         // Base wildcard
//         let result = intersect_with_offset(&None, &Some(vec![10, 20, 30]), 5, 0, values_per_map);
//         assert_eq!(result, Some(vec![5, 15, 25]));

//         // Next wildcard
//         let result = intersect_with_offset(&Some(vec![10, 20, 30]), &None, 5, 0, values_per_map);
//         assert_eq!(result, Some(vec![10, 20, 30]));

//         // Normal intersection
//         let result = intersect_with_offset(
//             &Some(vec![10, 20, 30, 40]),
//             &Some(vec![15, 25, 35, 45]),
//             5,
//             0,
//             values_per_map,
//         );
//         assert_eq!(result, Some(vec![10, 20, 30, 40]));

//         // No intersection
//         let result = intersect_with_offset(
//             &Some(vec![10, 20, 30]),
//             &Some(vec![100, 200]),
//             5,
//             0,
//             values_per_map,
//         );
//         assert_eq!(result, Some(vec![]));
//     }
// }
