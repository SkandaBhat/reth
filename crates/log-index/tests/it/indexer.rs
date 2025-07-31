use crate::storage::InMemoryFilterMapsProvider;
use alloy_primitives::BlockNumber;
use reth_ethereum_primitives::Receipt;
use reth_log_index::{
    extract_log_values_from_block, FilterMapAccumulator, FilterMapParams, FilterMapsReader,
    FilterMapsWriter, FilterResult,
};
use reth_provider::test_utils::MockEthProvider;
use reth_provider::{BlockReader, ReceiptProvider};
use std::ops::RangeInclusive;
use std::sync::Arc;

fn persist(
    accumulator: &mut FilterMapAccumulator,
    storage: &InMemoryFilterMapsProvider,
) -> FilterResult<()> {
    for completed_map in accumulator.drain_completed_maps() {
        // store filter map rows
        let rows =
            completed_map.rows.iter().map(|(row_index, row)| (*row_index, row.clone())).collect();
        storage.store_filter_map_rows(completed_map.index, rows)?;

        // store block delimiters
        for delimiter in completed_map.delimiters {
            storage.store_block_delimiter(delimiter)?;
        }
    }

    // store partial map
    if let Some((map_index, map)) = accumulator.take_partial_map() {
        let rows = map.rows.iter().map(|(row_index, row)| (*row_index, row.clone())).collect();
        storage.store_filter_map_rows(map_index, rows)?;

        // store block delimiters
        for delimiter in map.delimiters {
            storage.store_block_delimiter(delimiter)?;
        }
    }

    Ok(())
}

pub(crate) async fn index(
    provider: Arc<MockEthProvider>,
    range: RangeInclusive<BlockNumber>,
    storage: Arc<InMemoryFilterMapsProvider>,
) -> FilterResult<()> {
    let params = FilterMapParams::default();

    // Get starting position from storage
    let first_block_delimiter = storage.get_block_delimiter(*range.start()).unwrap_or_default();
    let log_value_index = first_block_delimiter.unwrap_or_default().log_value_index;
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
        .for_each(|(delimiter, log_values)| {
            if let Some(delimiter) = delimiter {
                let _ = accumulator
                    .add_block_delimiter(delimiter)
                    .map_err(|e| eprintln!("Error adding block delimiter: {:?}", e));
            }

            for log_value in log_values {
                let _ = accumulator
                    .add_log_value(log_value.value)
                    .map_err(|e| eprintln!("Error adding log value: {:?}", e));
            }
        });

    // write filter maps and block delimiters to storage
    let _ = persist(&mut accumulator, &storage).map_err(|e| eprintln!("Error persisting: {:?}", e));

    Ok(())
}
