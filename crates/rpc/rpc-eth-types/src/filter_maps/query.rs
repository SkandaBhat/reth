//! Main query interface for FilterMaps.
//!
//! This module provides the high-level API for querying logs using FilterMaps.

use crate::filter_maps::{
    matcher::{FilterMapProvider, Matcher},
    types::{FilterError, FilterResult},
};
use alloy_primitives::{Address, BlockNumber, B256};
use alloy_rpc_types_eth::Log;
use std::sync::Arc;

/// Performs a log query using FilterMaps.
///
/// This is the main entry point for log filtering using the FilterMaps data structure.
/// It constructs the appropriate matcher hierarchy and processes the query efficiently.
///
/// # Arguments
///
/// * `provider` - The provider for accessing FilterMaps data
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
/// - There's an issue accessing the FilterMaps data
pub fn query_logs(
    provider: Arc<dyn FilterMapProvider>,
    first_block: BlockNumber,
    last_block: BlockNumber,
    addresses: Vec<Address>,
    topics: Vec<Vec<B256>>,
) -> FilterResult<Vec<Log>> {
    let params = provider.params();

    // Get log index range
    let first_index = provider.block_to_log_index(first_block)?;
    let last_index = if last_block == u64::MAX {
        u64::MAX
    } else {
        let next_block_index = provider.block_to_log_index(last_block + 1)?;
        if next_block_index > 0 {
            next_block_index - 1
        } else {
            return Ok(vec![]);
        }
    };

    if last_index < first_index {
        return Ok(vec![]);
    }

    // Build matcher hierarchy
    let matcher = build_matcher(provider.clone(), addresses, topics)?;

    // Calculate map indices to search
    let first_map = (first_index >> params.log_values_per_map) as u32;
    let last_map = (last_index >> params.log_values_per_map) as u32;
    let map_indices: Vec<u32> = (first_map..=last_map).collect();

    // Process the query
    let results = matcher.process(&map_indices)?;

    // Collect logs from matches
    let mut logs = Vec::new();
    for result in results {
        if let Some(matches) = result.matches {
            for log_index in matches {
                // Filter out matches outside our range
                if log_index < first_index || log_index > last_index {
                    continue;
                }

                if let Some(log) = provider.get_log(log_index)? {
                    logs.push(log);
                }
            }
        } else {
            // Wildcard match - this means the filter matches everything
            return Err(FilterError::MatchAll.into());
        }
    }

    Ok(logs)
}

/// Builds the matcher hierarchy from filter criteria.
fn build_matcher(
    provider: Arc<dyn FilterMapProvider>,
    addresses: Vec<Address>,
    topics: Vec<Vec<B256>>,
) -> FilterResult<Matcher> {
    let params = provider.params();
    let mut matchers = Vec::with_capacity(topics.len() + 1);

    // Address matcher (position 0)
    if addresses.is_empty() {
        // Empty address list = match any address
        matchers.push(Matcher::any(vec![]));
    } else {
        // Match any of the specified addresses
        let address_matchers: Vec<_> = addresses
            .into_iter()
            .map(|addr| {
                let value = address_to_log_value(addr);
                Matcher::single(provider.clone(), value)
            })
            .collect();
        matchers.push(Matcher::any(address_matchers));
    }

    // Topic matchers (positions 1+)
    for topic_list in topics {
        if topic_list.is_empty() {
            // Empty topic list = match any topic at this position
            matchers.push(Matcher::any(vec![]));
        } else {
            // Match any of the specified topics
            let topic_matchers: Vec<_> = topic_list
                .into_iter()
                .map(|topic| {
                    let value = topic_to_log_value(topic);
                    Matcher::single(provider.clone(), value)
                })
                .collect();
            matchers.push(Matcher::any(topic_matchers));
        }
    }

    // Create sequence matcher for the entire filter
    Ok(Matcher::sequence_from_slice(Arc::new(params.clone()), matchers))
}

/// Converts an address to a log value hash.
pub fn address_to_log_value(address: Address) -> B256 {
    // In the log value encoding, addresses are stored as the hash of the address
    // with a specific prefix to distinguish them from topics
    let mut buf = [0u8; 32];
    buf[0] = 0x00; // Address prefix
    buf[12..32].copy_from_slice(address.as_slice());
    B256::from(buf)
}

/// Converts a topic to a log value hash.
pub fn topic_to_log_value(topic: B256) -> B256 {
    // Topics are stored directly as their hash value
    topic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_to_log_value() {
        let addr = Address::from([0x12; 20]);
        let value = address_to_log_value(addr);
        assert_eq!(value[0], 0x00); // Address prefix
        assert_eq!(&value[12..32], addr.as_slice());
    }

    #[test]
    fn test_topic_to_log_value() {
        let topic = B256::from([0x42; 32]);
        let value = topic_to_log_value(topic);
        assert_eq!(value, topic);
    }
}

#[cfg(test)]
#[path = "query_test.rs"]
mod query_test;
