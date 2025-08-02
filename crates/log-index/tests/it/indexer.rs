use crate::storage::InMemoryFilterMapsProvider;
use alloy_primitives::BlockNumber;
use reth_ethereum_primitives::Receipt;
use reth_log_index::{
    extract_log_values_from_block, FilterMapAccumulator, FilterMapMetadata, FilterMapParams,
    FilterMapsReader, FilterMapsWriter, FilterResult,
};
use reth_provider::test_utils::MockEthProvider;
use reth_provider::{BlockReader, ReceiptProvider};
use std::ops::RangeInclusive;
use std::sync::Arc;

fn persist(
    accumulator: &mut FilterMapAccumulator,
    storage: &InMemoryFilterMapsProvider,
) -> FilterResult<()> {
    let mut last_indexed_block = 0;
    let mut last_map_index = 0;
    for completed_map in accumulator.drain_completed_maps() {
        let completed_map_clone = completed_map.clone();
        println!("storing filter map: {:?}", completed_map.index);
        // store filter map rows

        storage.store_filter_map_rows(completed_map.rows)?;

        // store block log value indices
        for (block_number, log_value_index) in completed_map.block_log_value_indices {
            storage.store_log_value_index_for_block(block_number, log_value_index)?;
        }
        last_indexed_block =
            *completed_map_clone.block_log_value_indices.keys().max().unwrap_or(&0);
        last_map_index = completed_map_clone.index;
    }

    // store metadata
    let mut metadata = storage.get_metadata()?.unwrap_or_default();
    metadata.last_indexed_block = last_indexed_block;
    metadata.last_map_index = last_map_index;
    metadata.next_log_value_index = accumulator.log_value_index + 1;

    storage.store_metadata(metadata).unwrap();

    Ok(())
}

pub(crate) async fn index(
    provider: Arc<MockEthProvider>,
    range: RangeInclusive<BlockNumber>,
    storage: Arc<InMemoryFilterMapsProvider>,
) -> FilterResult<()> {
    let params = FilterMapParams::default();

    // Get starting position from storage
    let log_value_index =
        storage.get_log_value_index_for_block(*range.start())?.unwrap_or_default();
    let map_index = log_value_index >> params.log_values_per_map;
    let mut accumulator = FilterMapAccumulator::new(params.clone(), map_index, log_value_index);

    let blocks = provider.block_range(range.clone()).unwrap_or_default();
    let receipts: Vec<Vec<Receipt>> =
        provider.receipts_by_block_range(range.clone()).unwrap_or_default();

    // Process all blocks to extract log values
    blocks
        .into_iter()
        .zip(receipts)
        .map(|(block, receipts)| extract_log_values_from_block(block, receipts))
        .for_each(|(block_delimiter, log_values)| {
            let _ = accumulator
                .process_block(block_delimiter, log_values)
                .map_err(|e| eprintln!("Error processing block: {:?}", e));
        });

    // write filter maps and block log value indices to storage
    let _ = persist(&mut accumulator, &storage).map_err(|e| eprintln!("Error persisting: {:?}", e));

    Ok(())
}
