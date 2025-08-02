//! Main query interface for `FilterMaps`.
//!
//! This module provides the high-level API for querying logs using `FilterMaps`.

use crate::{
    types::{FilterError, FilterResult},
    utils::{address_value, topic_value},
};
use alloy_primitives::{Address, BlockNumber, Log, B256};
use std::{ops::RangeInclusive, sync::Arc};

use crate::{storage::FilterMapRow, FilterMapParams, FilterMapsReader};

/// Performs a log query using `FilterMaps`.
///
/// This is the main entry point for log filtering using the `FilterMaps` data structure.
/// It constructs the appropriate matcher hierarchy and processes the query efficiently.
///
/// # Arguments
///
/// * `provider` - The provider for accessing `FilterMaps` data
/// * `first_block` - The first block to search (inclusive)
/// * `last_block` - The last block to search (inclusive)
/// * `addresses` - List of addresses to match (empty = match all)
/// * `topics` - List of topic filters, where each position can have multiple values (OR)
///
/// # Returns
///
/// A list of logs matching the filter criteria. Note that false positives are possible
/// and should be filtered out by the caller if exact matching is required.
///
/// # Errors
///
/// Returns an error if:
/// - The filter matches all logs (use legacy filtering instead)
/// - There's an issue accessing the `FilterMaps` data
pub fn query_logs<P: FilterMapsReader + Clone + 'static>(
    provider: Arc<P>,
    range: RangeInclusive<BlockNumber>,
    address: Address,
    topics: Vec<B256>,
) -> FilterResult<Vec<u64>> {
    let params = FilterMapParams::default(); // TODO: figure out a better way to get the params

    let delimiter_start = provider.get_block_delimiter(*range.start())?.unwrap_or_default();

    let first_index =
        if delimiter_start.parent_block_number > 0 { delimiter_start.log_value_index } else { 0 };

    // For the end block, we need the delimiter of the NEXT block to know where it ends
    let last_index =
        provider.get_block_delimiter(*range.end())?.unwrap_or_default().log_value_index;

    println!("first_index: {:?}, last_index: {:?}", first_index, last_index);

    // Calculate map indices to search
    let first_map = first_index >> params.log_values_per_map;
    let last_map = last_index >> params.log_values_per_map;
    let map_indices: Vec<u64> = (first_map..=last_map).collect();

    println!("map_indices: {:?}", map_indices);

    let log_indices =
        get_log_value_indices(provider.clone(), &map_indices, &address, &topics, &params)?;

    Ok(log_indices)
}

fn get_log_value_indices<P: FilterMapsReader + Clone + 'static>(
    provider: Arc<P>,
    map_indices: &[u64],
    address: &Address,
    topics: &[B256],
    params: &FilterMapParams,
) -> FilterResult<Vec<u64>> {
    let address_value = address_value(&address);
    let address_matches =
        simple_single_value_matcher(provider.as_ref(), &map_indices, &address_value, &params)?;

    if address_matches.is_empty() {
        return Ok(Vec::new());
    }

    let mut log_indices = Vec::new();
    println!("address_matches: {:?}", address_matches);

    'address_loop: for address_match in address_matches {
        for (i, topic) in topics.iter().enumerate() {
            let topic_index = address_match + 1 + i as u64;
            let topic_value = topic_value(&topic);
            if !verify_index_matches(provider.as_ref(), &topic_value, topic_index, &params)? {
                println!(
                    "skipping because topic_index: {:?} does not match topic_value: {:?}",
                    topic_index, topic_value
                );
                continue 'address_loop;
            }
        }

        log_indices.push(address_match);
    }

    Ok(log_indices)
}

pub fn simple_single_value_matcher<P: FilterMapsReader + 'static>(
    provider: &P,
    map_indices: &[u64],
    log_value: &B256,
    params: &FilterMapParams,
) -> FilterResult<Vec<u64>> {
    let mut all_matches = Vec::new();

    for &map_index in map_indices {
        // Get potential matches for this map following EIP-7745 spec
        let rows = get_filter_map_rows(provider, map_index, log_value, params)?;
        // Now use the existing potential_matches function to find matches
        let matches = params.potential_matches(&rows, map_index, log_value)?;
        all_matches.extend(matches);
    }

    // Sort and deduplicate results
    all_matches.sort_unstable();
    all_matches.dedup();

    Ok(all_matches)
}

fn verify_index_matches<P: FilterMapsReader + 'static>(
    provider: &P,
    log_value: &B256,
    expected_log_value_index: u64,
    params: &FilterMapParams,
) -> FilterResult<bool> {
    let map_index = expected_log_value_index >> params.log_values_per_map;

    let rows = get_filter_map_rows(provider, map_index, log_value, params)?;

    let expected_column_index = params.column_index(expected_log_value_index, log_value);

    for (i, row) in rows.iter().enumerate() {
        let max_len = params.max_row_length(i as u64) as usize;
        let row_len = row.columns.len().min(max_len);

        for &column_value in &row.columns[..row_len] {
            if column_value == expected_column_index {
                return Ok(true);
            }
        }

        if row_len < max_len {
            break;
        }
    }

    Ok(false)
}

fn get_filter_map_rows<P: FilterMapsReader + 'static>(
    provider: &P,
    map_index: u64,
    log_value: &B256,
    params: &FilterMapParams,
) -> FilterResult<Vec<FilterMapRow>> {
    let mut rows = Vec::new();
    let mut layer_index = 0;

    loop {
        // Calculate row index for this layer
        let row_index = params.row_index(map_index, layer_index, log_value);

        // Fetch the row
        let row_vec = provider.get_filter_map_rows(map_index, &[row_index])?;
        if row_vec.is_empty() {
            break;
        }
        let row = &row_vec[0];

        // Calculate max row length for this layer
        let max_row_length = params.max_row_length(layer_index) as usize;
        let row_len = row.columns.len().min(max_row_length);

        // Check if we need this row for potential_matches
        rows.push(row.clone());

        // If row isn't full, we're done with layers for this map
        if row_len < max_row_length {
            break;
        }

        layer_index += 1;
        if layer_index >= crate::constants::MAX_LAYERS {
            return Err(FilterError::MaxLayersExceeded(crate::constants::MAX_LAYERS));
        }
    }

    Ok(rows)
}
