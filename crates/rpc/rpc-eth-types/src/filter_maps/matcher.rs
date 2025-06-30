//! Matcher types for querying FilterMaps based on EIP-7745.

use alloy_primitives::B256;
use alloy_rpc_types_eth::Filter;
use std::collections::HashSet;

use super::{address_value, topic_value, FilterMapParams};

/// A matcher for querying filter maps.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterMatcher {
    /// Matches a single log value (address or topic)
    Single(SingleMatcher),
    /// Matches any of the contained matchers (OR)
    Any(Vec<FilterMatcher>),
    /// Matches a sequence of log values (AND for consecutive values)
    Sequence(Vec<FilterMatcher>),
}

/// Matches a single log value.
#[derive(Debug, Clone, PartialEq)]
pub struct SingleMatcher {
    /// The hash value to match
    pub value: B256,
    /// Map indices where this value should be searched
    pub map_indices: Vec<u32>,
}

/// Result of a matcher query.
#[derive(Debug, Clone)]
pub struct MatcherResult {
    /// Set of log value indices that potentially match
    pub potential_matches: HashSet<u64>,
    /// Whether this is a wildcard match (matches all)
    pub is_wildcard: bool,
}

impl FilterMatcher {
    /// Create matchers from a filter for a given block range.
    pub fn from_filter(
        filter: &Filter,
        from_block: u64,
        to_block: u64,
        params: &FilterMapParams,
    ) -> Vec<FilterMatcher> {
        let mut matchers = Vec::new();

        // Calculate map indices for the block range
        let from_map = (from_block / params.values_per_map()) as u32;
        let to_map = (to_block / params.values_per_map()) as u32;
        let map_indices: Vec<u32> = (from_map..=to_map).collect();

        // Address matchers
        if !filter.address.is_empty() {
            let address_matchers: Vec<FilterMatcher> = filter
                .address
                .iter()
                .map(|addr| {
                    FilterMatcher::Single(SingleMatcher {
                        value: address_value(addr),
                        map_indices: map_indices.clone(),
                    })
                })
                .collect();

            if address_matchers.len() == 1 {
                matchers.push(address_matchers.into_iter().next().unwrap());
            } else {
                matchers.push(FilterMatcher::Any(address_matchers));
            }
        }

        // Topic matchers
        let mut topic_sequence = Vec::new();
        for topic_filter in &filter.topics {
            if topic_filter.is_empty() {
                // Empty FilterSet means wildcard - matches any value at this position
                topic_sequence.push(None);
            } else {
                let topics: Vec<_> = topic_filter.iter().collect();
                if topics.len() == 1 {
                    // Single topic value
                    topic_sequence.push(Some(FilterMatcher::Single(SingleMatcher {
                        value: topic_value(topics[0]),
                        map_indices: map_indices.clone(),
                    })));
                } else {
                    // Multiple topic values (OR)
                    let topic_matchers: Vec<FilterMatcher> = topics
                        .iter()
                        .map(|topic| {
                            FilterMatcher::Single(SingleMatcher {
                                value: topic_value(topic),
                                map_indices: map_indices.clone(),
                            })
                        })
                        .collect();
                    topic_sequence.push(Some(FilterMatcher::Any(topic_matchers)));
                }
            }
        }

        // If we have topics, create a sequence matcher
        if !topic_sequence.is_empty() && topic_sequence.iter().any(|t| t.is_some()) {
            // Filter out trailing None values
            while topic_sequence.last() == Some(&None) {
                topic_sequence.pop();
            }

            // Convert to sequence if we have any non-wildcard topics
            let sequence: Vec<FilterMatcher> =
                topic_sequence.into_iter().filter_map(|t| t).collect();

            if !sequence.is_empty() {
                if sequence.len() == 1 {
                    matchers.push(sequence.into_iter().next().unwrap());
                } else {
                    matchers.push(FilterMatcher::Sequence(sequence));
                }
            }
        }

        matchers
    }

    /// Check if this matcher is a wildcard (matches everything).
    pub fn is_wildcard(&self) -> bool {
        match self {
            FilterMatcher::Single(_) => false,
            FilterMatcher::Any(matchers) => matchers.is_empty(),
            FilterMatcher::Sequence(matchers) => matchers.is_empty(),
        }
    }
}

impl SingleMatcher {
    /// Create a new single matcher.
    pub fn new(value: B256, map_indices: Vec<u32>) -> Self {
        Self { value, map_indices }
    }
}

impl MatcherResult {
    /// Create a new wildcard result that matches everything.
    pub fn wildcard() -> Self {
        Self { potential_matches: HashSet::new(), is_wildcard: true }
    }

    /// Create a result with specific matches.
    pub fn with_matches(matches: HashSet<u64>) -> Self {
        Self { potential_matches: matches, is_wildcard: false }
    }

    /// Combine two results with AND logic.
    pub fn and(self, other: Self) -> Self {
        match (self.is_wildcard, other.is_wildcard) {
            (true, true) => Self::wildcard(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => {
                let intersection: HashSet<u64> = self
                    .potential_matches
                    .intersection(&other.potential_matches)
                    .cloned()
                    .collect();
                Self::with_matches(intersection)
            }
        }
    }

    /// Combine two results with OR logic.
    pub fn or(self, other: Self) -> Self {
        match (self.is_wildcard, other.is_wildcard) {
            (true, _) | (_, true) => Self::wildcard(),
            (false, false) => {
                let mut union = self.potential_matches;
                union.extend(other.potential_matches);
                Self::with_matches(union)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_rpc_types_eth::{FilterBlockOption, FilterSet};

    #[test]
    fn test_single_address_matcher() {
        let params = FilterMapParams::default();
        let filter = Filter {
            block_option: FilterBlockOption::Range { from_block: None, to_block: None },
            address: vec![address!("0000000000000000000000000000000000000001")].into(),
            topics: Default::default(),
        };

        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);

        match &matchers[0] {
            FilterMatcher::Single(sm) => {
                assert_eq!(sm.value, address_value(&filter.address.iter().next().unwrap()));
                assert_eq!(sm.map_indices.len(), 1); // Maps 0
            }
            _ => panic!("Expected single matcher"),
        }
    }

    #[test]
    fn test_multiple_address_matcher() {
        let params = FilterMapParams::default();
        let filter = Filter {
            block_option: FilterBlockOption::Range { from_block: None, to_block: None },
            address: vec![
                address!("0000000000000000000000000000000000000001"),
                address!("0000000000000000000000000000000000000002"),
            ]
            .into(),
            topics: Default::default(),
        };

        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);

        match &matchers[0] {
            FilterMatcher::Any(any_matchers) => {
                assert_eq!(any_matchers.len(), 2);
            }
            _ => panic!("Expected Any matcher"),
        }
    }

    #[test]
    fn test_topic_sequence_matcher() {
        let params = FilterMapParams::default();
        let topic1 = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let topic2 = b256!("0000000000000000000000000000000000000000000000000000000000000002");

        let filter = Filter {
            block_option: FilterBlockOption::Range { from_block: None, to_block: None },
            address: vec![].into(),
            topics: [
                vec![topic1].into(),
                vec![topic2].into(),
                FilterSet::default(),
                FilterSet::default(),
            ],
        };

        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);

        match &matchers[0] {
            FilterMatcher::Sequence(seq) => {
                assert_eq!(seq.len(), 2);
            }
            _ => panic!("Expected Sequence matcher"),
        }
    }

    #[test]
    fn test_wildcard_topics() {
        let params = FilterMapParams::default();
        let topic2 = b256!("0000000000000000000000000000000000000000000000000000000000000002");

        // Filter with wildcard first topic and specific second topic
        let filter = Filter {
            block_option: FilterBlockOption::Range { from_block: None, to_block: None },
            address: vec![].into(),
            topics: [Default::default(), topic2.into(), Default::default(), Default::default()],
        };

        let matchers = FilterMatcher::from_filter(&filter, 0, 1000, &params);
        assert_eq!(matchers.len(), 1);

        // Should only have the second topic as a matcher
        match &matchers[0] {
            FilterMatcher::Single(sm) => {
                assert_eq!(sm.value, topic_value(&topic2));
            }
            _ => panic!("Expected Single matcher for topic2"),
        }
    }

    #[test]
    fn test_matcher_result_operations() {
        let mut set1 = HashSet::new();
        set1.insert(1);
        set1.insert(2);
        set1.insert(3);

        let mut set2 = HashSet::new();
        set2.insert(2);
        set2.insert(3);
        set2.insert(4);

        let result1 = MatcherResult::with_matches(set1);
        let result2 = MatcherResult::with_matches(set2);

        // Test AND
        let and_result = result1.clone().and(result2.clone());
        assert_eq!(and_result.potential_matches.len(), 2); // {2, 3}
        assert!(and_result.potential_matches.contains(&2));
        assert!(and_result.potential_matches.contains(&3));

        // Test OR
        let or_result = result1.or(result2);
        assert_eq!(or_result.potential_matches.len(), 4); // {1, 2, 3, 4}

        // Test wildcard AND
        let wildcard = MatcherResult::wildcard();
        let and_wildcard = wildcard.and(MatcherResult::with_matches(HashSet::from([5, 6])));
        assert_eq!(and_wildcard.potential_matches.len(), 2);
        assert!(!and_wildcard.is_wildcard);
    }
}
