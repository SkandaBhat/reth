#[cfg(test)]
mod tests {
    use crate::filter_maps::{
        constants::DEFAULT_PARAMS,
        matcher::{FilterMapProvider, Matcher},
        params::FilterMapParams,
        query::{address_to_log_value, topic_to_log_value},
        FilterRow,
    };
    use alloy_primitives::{address, BlockNumber, B256};
    use alloy_rpc_types_eth::Log;
    use reth_errors::ProviderResult;
    use std::{collections::HashMap, sync::Arc};

    #[derive(Debug)]
    struct SimpleProvider {
        params: FilterMapParams,
        rows: HashMap<(u32, u32, u32), FilterRow>,
    }

    impl SimpleProvider {
        fn new() -> Self {
            Self {
                params: DEFAULT_PARAMS.clone(),
                rows: HashMap::new(),
            }
        }

        fn add_row(&mut self, map_index: u32, row_index: u32, layer: u32, cols: Vec<u32>) {
            self.rows.insert((map_index, row_index, layer), cols);
        }
    }

    impl FilterMapProvider for SimpleProvider {
        fn params(&self) -> &FilterMapParams {
            &self.params
        }

        fn block_to_log_index(&self, _block_number: BlockNumber) -> ProviderResult<u64> {
            Ok(0)
        }

        fn get_filter_rows(
            &self,
            map_indices: &[u32],
            row_index: u32,
            layer: u32,
        ) -> ProviderResult<Vec<FilterRow>> {
            let mut result = Vec::with_capacity(map_indices.len());
            for &map_index in map_indices {
                let row = self
                    .rows
                    .get(&(map_index, row_index, layer))
                    .cloned()
                    .unwrap_or_default();
                result.push(row);
            }
            Ok(result)
        }

        fn get_log(&self, _log_index: u64) -> ProviderResult<Option<Log>> {
            Ok(None)
        }
    }

    #[test]
    fn test_any_matcher_basic() {
        let mut provider = SimpleProvider::new();
        let addr1 = address!("0x1111111111111111111111111111111111111111");
        let addr2 = address!("0x2222222222222222222222222222222222222222");
        
        let addr1_value = address_to_log_value(addr1);
        let addr2_value = address_to_log_value(addr2);
        
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
            Matcher::single(provider_arc.clone(), addr2_value),
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
        let addr_value = address_to_log_value(addr);
        
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
        
        let addr_value = address_to_log_value(addr);
        let topic_value = topic_to_log_value(topic);
        
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
            Matcher::single(provider_arc.clone(), topic_value),
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
}