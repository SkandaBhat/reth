use crate::{
    utils::{address_value, topic_value},
    FilterMapParams, FilterMapsReader, FilterResult,
};
use alloy_primitives::{map::HashMap, B256};
use alloy_rpc_types_eth::Filter;
use std::sync::Arc;
use tracing::{error, info};

/// Result from a filter map query containing indices and metadata
#[derive(Debug, Clone)]
pub struct FilterMapQueryResult {
    /// Log value index from the filter map
    pub log_index: u64,
    /// Block number containing this log
    pub block_number: u64,
    /// Starting log value index for this block (for position calculation)
    pub block_start_lv_index: u64,
}

/// Query logs from filter maps for a given block range
pub fn query_logs_in_block_range<P>(
    provider: Arc<P>,
    params: &FilterMapParams,
    filter: &Filter,
    from_block: u64,
    to_block: u64,
) -> FilterResult<Vec<FilterMapQueryResult>>
where
    P: FilterMapsReader + Send + Sync + 'static,
{
    // Extract filter values
    let addr_vals: Vec<B256> = filter.address.iter().map(address_value).collect();

    let topic_vals: Vec<(usize, Vec<B256>)> = filter
        .topics
        .iter()
        .enumerate()
        .filter_map(|(i, topics)| {
            (!topics.is_empty()).then(|| (i, topics.iter().map(topic_value).collect()))
        })
        .collect();

    let log_value_indices = provider.get_log_value_indices_range(from_block..=to_block).unwrap();
    let mut log_value_indices_map = HashMap::new();
    for index in &log_value_indices {
        log_value_indices_map.insert(index.block_number, index.log_value_index);
    }

    // Get map indices for the block range
    let first_block_lv = log_value_indices.first().map(|index| index.log_value_index).unwrap_or(0);
    let last_block_lv = log_value_indices.last().map(|index| index.log_value_index).unwrap_or(0);

    // calculate map range
    let shift_bits = params.log_values_per_map;
    let first_map = (first_block_lv >> shift_bits) as u32;
    let last_map = (last_block_lv >> shift_bits) as u32;

    let map_indices = (first_map..=last_map).collect::<Vec<_>>();

    // prepare values for query
    let mut all_values: Vec<&B256> = Vec::new();
    all_values.extend(addr_vals.iter());
    for (_, vals) in &topic_vals {
        all_values.extend(vals.iter());
    }

    // fetch all rows at once
    let rows_by_map_value =
        provider.get_filter_map_rows(from_block..=to_block, &all_values).unwrap();
    let mut rows_by_map_value_map = HashMap::new();
    for row in rows_by_map_value {
        rows_by_map_value_map.insert((row.map_index, row.value), row);
    }

    // Query each map inline (merged from query_single_map)
    let mut results = Vec::new();
    for map_index in map_indices {
        // Start with address matches (position 0)
        let mut candidates: Vec<u64> = Vec::new();

        // Collect all then sort/dedup
        if !addr_vals.is_empty() {
            for val in &addr_vals {
                if let Some(rows) = rows_by_map_value_map.get(&(map_index, *val)) {
                    let matches = params.potential_matches(&rows.layers, map_index, val);
                    candidates.extend(matches);
                }
            }

            // Sort and dedup once at the end
            candidates.sort_unstable();
            candidates.dedup();
        }

        // Now filter by each topic position sequentially
        for (position, topic_values) in &topic_vals {
            let offset = 1u64 + (*position as u64);
            let mut next_candidates = Vec::new();

            // Cache potential matches
            let mut match_cache: HashMap<(u32, &B256), Vec<u64>> = HashMap::default();

            // For each current candidate, check if any topic matches at position+offset
            for &candidate in &candidates {
                let check_index = candidate + offset;

                for val in topic_values {
                    // Check current map
                    let matches = match_cache.entry((map_index, val)).or_insert_with(|| {
                        rows_by_map_value_map
                            .get(&(map_index, *val))
                            .map(|rows| params.potential_matches(&rows.layers, map_index, val))
                            .unwrap_or_default()
                    });

                    if matches.binary_search(&check_index).is_ok() {
                        next_candidates.push(candidate);
                        break;
                    }

                    // Check forward spill if needed
                    let next_map_index = map_index + 1;
                    if check_index >= ((next_map_index as u64) << shift_bits) {
                        let spill_matches =
                            match_cache.entry((next_map_index, val)).or_insert_with(|| {
                                rows_by_map_value_map
                                    .get(&(next_map_index, *val))
                                    .map(|rows| {
                                        params.potential_matches(&rows.layers, next_map_index, val)
                                    })
                                    .unwrap_or_default()
                            });

                        if spill_matches.binary_search(&check_index).is_ok() {
                            next_candidates.push(candidate);
                            break;
                        }
                    }
                }
            }

            candidates = next_candidates;
            if candidates.is_empty() {
                break; // No matches at this topic position
            }
        }

        for log_index in candidates {
            // Use binary search to find the block containing this log index
            // We want the last block where log_value_index <= log_index
            let block_idx =
                match log_value_indices.binary_search_by_key(&log_index, |lv| lv.log_value_index) {
                    Ok(idx) => idx,      // Exact match - log is at the start of this block
                    Err(0) => continue,  // Before our first block
                    Err(idx) => idx - 1, // Log belongs to the previous block
                };

            let block_number = log_value_indices[block_idx].block_number;

            // Check if block is in range
            if block_number < from_block || block_number > to_block {
                error!("block not in range: {}", block_number);
                continue;
            }

            let block_start_lv_index = log_value_indices[block_idx].log_value_index;

            results.push(FilterMapQueryResult { log_index, block_number, block_start_lv_index });
        }
    }

    Ok(results)
}
