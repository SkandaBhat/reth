use crate::storage::InMemoryFilterMapsProvider;
use alloy_primitives::BlockNumber;
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_filter_maps::{FilterMapParams, FilterMapsProcessor, FilterMapsWriter, FilterResult};
use reth_provider::test_utils::MockEthProvider;
use reth_provider::{BlockReader, ReceiptProvider};
use std::ops::RangeInclusive;
use std::sync::Arc;

pub(crate) async fn index(
    provider: Arc<MockEthProvider>,
    range: RangeInclusive<BlockNumber>,
    storage: Arc<InMemoryFilterMapsProvider>,
) -> FilterResult<()> {
    let params = FilterMapParams::default();

    let mut indexer = FilterMapsProcessor::new(params.clone(), storage.as_ref().clone());

    let range_vec: Vec<_> = range.clone().collect();
    for (_, block_number) in range_vec.iter().enumerate() {
        let block = provider
            .block(BlockHashOrNumber::Number(*block_number))
            .unwrap_or_default()
            .unwrap_or_default();
        let receipts = provider
            .receipts_by_block(BlockHashOrNumber::Number(*block_number))
            .unwrap_or_default()
            .unwrap_or_default();
        indexer.process_block(block.number, block.hash_slow(), &receipts).unwrap();
    }

    // Finalize any remaining map that hasn't been finalized yet
    if let Some(map) = indexer.finalize_current_map()? {
        // The map is already stored by finalize_current_map, so we don't need to do anything
        println!("Finalized last map with index: {}", map.map_index);
    }

    // Update the filter maps range to reflect what we've indexed
    let (first_block, last_block) = indexer.indexed_range();
    let current_lv_index = indexer.current_lv_index();
    let range_metadata = reth_filter_maps::storage::FilterMapsRange {
        head_indexed: true,
        blocks_first: first_block,
        blocks_after_last: last_block + 1,
        head_delimiter: current_lv_index,
        maps_first: 0,
        maps_after_last: (current_lv_index >> params.log_values_per_map) + 1,
        tail_partial_epoch: 0,
        version: 1,
    };
    storage.as_ref().update_filter_maps_range(range_metadata)?;

    Ok(())
}
