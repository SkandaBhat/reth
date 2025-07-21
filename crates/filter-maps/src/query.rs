//! Main query interface for `FilterMaps`.
//!
//! This module provides the high-level API for querying logs using `FilterMaps`.

use crate::{
    matcher::Matcher,
    provider::FilterMapProvider,
    types::{FilterError, FilterResult},
};
use alloy_primitives::{Address, BlockNumber, Log, B256};
use std::sync::Arc;

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
            return Err(FilterError::MatchAll);
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
            .map(|address| {
                let value = address_to_log_value(address);
                Matcher::single(provider.clone(), value)
            })
            .collect();
        matchers.push(Matcher::any(address_matchers));
    }

    // Add topic matchers
    for topic_list in topics {
        if !topic_list.is_empty() {
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
    // Use consistent SHA256 hashing as per EIP-7745
    crate::utils::address_value(&address)
}

/// Converts a topic to a log value hash.
pub fn topic_to_log_value(topic: B256) -> B256 {
    // Use consistent SHA256 hashing as per EIP-7745
    crate::utils::topic_value(&topic)
}

/// Verifies that a log actually matches the filter criteria.
///
/// This is used to filter out false positives from the probabilistic filter maps.
pub fn verify_log_matches_filter(log: &Log, addresses: &[Address], topics: &[Vec<B256>]) -> bool {
    // Check address match
    if !addresses.is_empty() && !addresses.contains(&log.address) {
        return false;
    }

    // Check topics match
    let log_topics = log.topics();
    for (i, topic_options) in topics.iter().enumerate() {
        if topic_options.is_empty() {
            // Empty means any topic at this position is ok
            continue;
        }

        // Check if log has a topic at this position
        if let Some(log_topic) = log_topics.get(i) {
            if !topic_options.contains(log_topic) {
                return false;
            }
        } else {
            // Log doesn't have a topic at this position but filter requires one
            return false;
        }
    }

    true
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_address_to_log_value() {
//         let addr = Address::from([0x12; 20]);
//         let value = address_to_log_value(addr);
//         // Should be SHA256 hash of the address
//         let expected = crate::utils::address_value(&addr);
//         assert_eq!(value, expected);

//         // Should be deterministic
//         let value2 = address_to_log_value(addr);
//         assert_eq!(value, value2);
//     }

//     #[test]
//     fn test_topic_to_log_value() {
//         let topic = B256::from([0x42; 32]);
//         let value = topic_to_log_value(topic);
//         // Should be SHA256 hash of the topic
//         let expected = crate::utils::topic_value(&topic);
//         assert_eq!(value, expected);

//         // Should be deterministic
//         let value2 = topic_to_log_value(topic);
//         assert_eq!(value, value2);
//     }

//     #[test]
//     fn test_verify_log_matches_filter() {
//         use alloy_primitives::{address, b256, Bytes, Log as PrimLog};

//         let addr1 = address!("1111111111111111111111111111111111111111");
//         let addr2 = address!("2222222222222222222222222222222222222222");
//         let topic1 = b256!("0000000000000000000000000000000000000000000000000000000000000001");
//         let topic2 = b256!("0000000000000000000000000000000000000000000000000000000000000002");
//         let topic3 = b256!("0000000000000000000000000000000000000000000000000000000000000003");

//         // Create a log with address and 2 topics
//         let inner_log = PrimLog::new(addr1, vec![topic1, topic2], Bytes::default()).unwrap();
//         let log = Log {
//             inner: inner_log,
//             block_hash: None,
//             block_number: None,
//             block_timestamp: None,
//             transaction_hash: None,
//             transaction_index: None,
//             log_index: None,
//             removed: false,
//         };

//         // Test exact match
//         assert!(verify_log_matches_filter(&log, &[addr1], &[vec![topic1], vec![topic2]]));

//         // Test address mismatch
//         assert!(!verify_log_matches_filter(&log, &[addr2], &[vec![topic1], vec![topic2]]));

//         // Test topic mismatch
//         assert!(!verify_log_matches_filter(&log, &[addr1], &[vec![topic3], vec![topic2]]));

//         // Test empty address filter (matches any)
//         assert!(verify_log_matches_filter(&log, &[], &[vec![topic1], vec![topic2]]));

//         // Test empty topic filter at position (matches any)
//         assert!(verify_log_matches_filter(&log, &[addr1], &[vec![], vec![topic2]]));

//         // Test multiple address options
//         assert!(verify_log_matches_filter(&log, &[addr2, addr1], &[vec![topic1], vec![topic2]]));

//         // Test multiple topic options
//         assert!(verify_log_matches_filter(&log, &[addr1], &[vec![topic3, topic1], vec![topic2]]));

//         // Test filter requires more topics than log has
//         assert!(!verify_log_matches_filter(
//             &log,
//             &[addr1],
//             &[vec![topic1], vec![topic2], vec![topic3]]
//         ));
//     }

//     #[test]
//     fn test_build_matcher_edge_cases() {
//         use crate::{constants::DEFAULT_PARAMS, FilterMapProvider};
//         use alloy_primitives::BlockNumber;
//         use alloy_rpc_types_eth::Log;
//         use reth_errors::ProviderResult;

//         struct MockProvider {
//             params: crate::FilterMapParams,
//         }

//         impl FilterMapProvider for MockProvider {
//             fn params(&self) -> &crate::FilterMapParams {
//                 &self.params
//             }

//             fn block_to_log_index(&self, _: BlockNumber) -> ProviderResult<u64> {
//                 Ok(0)
//             }

//             fn get_filter_rows(
//                 &self,
//                 map_indices: &[u32],
//                 _: u32,
//                 _: u32,
//             ) -> ProviderResult<Vec<crate::FilterRow>> {
//                 Ok(vec![vec![]; map_indices.len()])
//             }

//             fn get_log(&self, _: u64) -> ProviderResult<Option<Log>> {
//                 Ok(None)
//             }
//         }

//         let provider = Arc::new(MockProvider { params: DEFAULT_PARAMS });

//         // Test empty filter (should match all)
//         let _matcher = build_matcher(provider.clone(), vec![], vec![]).unwrap();
//         // The sequence matcher should have one ANY matcher for addresses

//         // Test with only addresses
//         let _matcher = build_matcher(provider.clone(), vec![Address::ZERO], vec![]).unwrap();
//         // Should create matcher for address only

//         // Test with addresses and topics
//         let _matcher = build_matcher(
//             provider,
//             vec![Address::ZERO],
//             vec![vec![B256::ZERO], vec![], vec![B256::from([1u8; 32])]],
//         )
//         .unwrap();
//         // Should create sequence of 4 matchers (1 address + 3 topics)
//     }
// }
