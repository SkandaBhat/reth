//! Matcher tests for filter maps

use alloy_primitives::{address, B256};
use reth_filter_maps::{address_value, topic_value, test_utils::SimpleProvider, FilterMapProvider, Matcher};
use std::sync::Arc;

#[test]
fn test_any_matcher_basic() {
    let mut provider = SimpleProvider::new();
    let addr1 = address!("0x1111111111111111111111111111111111111111");
    let addr2 = address!("0x2222222222222222222222222222222222222222");

    let addr1_value = address_value(&addr1);
    let addr2_value = address_value(&addr2);

    // Add rows for both addresses in map 0
    let addr1_row = provider.params.row_index(0, 0, &addr1_value);
    let addr1_col = provider.params.column_index(10, &addr1_value);
    provider.add_row(0, addr1_row, 0, vec![addr1_col]);

    let addr2_row = provider.params.row_index(0, 0, &addr2_value);
    let addr2_col = provider.params.column_index(20, &addr2_value);
    provider.add_row(0, addr2_row, 0, vec![addr2_col]);

    let provider_arc = Arc::new(provider);

    // Create ANY matcher for the two addresses
    let matcher = Matcher::any(vec![
        Matcher::single(provider_arc.clone(), addr1_value),
        Matcher::single(provider_arc, addr2_value),
    ]);

    // Process map 0
    let results = matcher.process(&[0]).unwrap();

    // We should get one result for map 0 with matches at indices 10 and 20
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].map_index, 0);

    let matches = results[0].matches.as_ref().unwrap();
    assert_eq!(matches.len(), 2);
    assert!(matches.contains(&10));
    assert!(matches.contains(&20));
}

#[test]
fn test_single_matcher_basic() {
    let mut provider = SimpleProvider::new();
    let addr = address!("0x1111111111111111111111111111111111111111");
    let addr_value = address_value(&addr);

    // Add row for address in map 0
    let row_idx = provider.params.row_index(0, 0, &addr_value);
    let col_idx = provider.params.column_index(42, &addr_value);
    provider.add_row(0, row_idx, 0, vec![col_idx]);

    let provider_arc = Arc::new(provider);

    // Create single matcher
    let matcher = Matcher::single(provider_arc, addr_value);

    // Process map 0
    let results = matcher.process(&[0]).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].map_index, 0);

    let matches = results[0].matches.as_ref().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0], 42);
}

#[test]
fn test_sequence_matcher_basic() {
    let mut provider = SimpleProvider::new();
    let addr = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let topic = B256::from([0x42; 32]);

    let addr_value = address_value(&addr);
    let topic_value = topic_value(&topic);

    // Add rows for address at index 50 and topic at index 51
    let addr_row = provider.params.row_index(0, 0, &addr_value);
    let addr_col = provider.params.column_index(50, &addr_value);
    provider.add_row(0, addr_row, 0, vec![addr_col]);

    let topic_row = provider.params.row_index(0, 0, &topic_value);
    let topic_col = provider.params.column_index(51, &topic_value);
    provider.add_row(0, topic_row, 0, vec![topic_col]);

    let provider_arc = Arc::new(provider);
    let params = Arc::new(provider_arc.params().clone());

    // Create sequence matcher for address followed by topic
    let matchers = vec![
        Matcher::single(provider_arc.clone(), addr_value),
        Matcher::single(provider_arc, topic_value),
    ];
    let matcher = Matcher::sequence_from_slice(params, matchers);

    // Process map 0
    let results = matcher.process(&[0]).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].map_index, 0);

    let matches = results[0].matches.as_ref().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0], 50); // Should match at the address position
}