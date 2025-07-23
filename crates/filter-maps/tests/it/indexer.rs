use crate::storage::InMemoryFilterMapsProvider;
use alloy_primitives::BlockNumber;
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_filter_maps::{
    extract_log_values_from_block, FilterMapAccumulator, FilterMapExt, FilterMapMetadata,
    FilterMapParams, FilterMapsReader, FilterMapsWriter, FilterResult,
};
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

    // Get starting position from storage
    let filter_maps_range = storage.get_filter_maps_range()?.unwrap_or_default();
    let mut log_value_index = filter_maps_range.head_delimiter;
    let map_index = (log_value_index >> params.log_values_per_map) as u32;

    let mut accumulator = FilterMapAccumulator::new(params.clone(), map_index);
    let mut completed_maps = Vec::new();
    let mut current_map_block_starts = Vec::new();
    let mut first_block = None;
    let mut last_block = None;

    let range_vec: Vec<_> = range.clone().collect();
    for block_number in range_vec.iter() {
        let block = provider
            .block(BlockHashOrNumber::Number(*block_number))
            .unwrap_or_default()
            .unwrap_or_default();
        let receipts = provider
            .receipts_by_block(BlockHashOrNumber::Number(*block_number))
            .unwrap_or_default()
            .unwrap_or_default();

        if first_block.is_none() {
            first_block = Some(*block_number);
        }
        last_block = Some(*block_number);

        // Get parent hash (for block delimiter)
        let parent_hash = if *block_number > 0 {
            provider
                .block(BlockHashOrNumber::Number(*block_number - 1))
                .unwrap_or_default()
                .map(|b| b.hash_slow())
                .unwrap_or_default()
        } else {
            Default::default()
        };

        // Handle genesis block special case
        if *block_number == 0 {
            // Genesis block has no delimiter, so store pointer at index 0
            current_map_block_starts.push((0, 0));
            storage.store_block_lv_pointer(0, 0)?;
        }

        // Extract log values from block
        let log_values = extract_log_values_from_block(
            log_value_index,
            block.number,
            block.hash_slow(),
            parent_hash,
            receipts,
        );

        // Process each log value
        for log_value in log_values {
            if log_value.is_block_delimiter {
                // Record block starting position
                current_map_block_starts.push((log_value.block_number + 1, log_value.index));
                storage.store_block_lv_pointer(log_value.block_number + 1, log_value.index)?;
                log_value_index = log_value.index + 1;

                // Check if we need to finalize current map
                if accumulator.should_finalize(log_value_index) {
                    let current_map_index = accumulator.map_index();
                    let metadata = FilterMapMetadata {
                        map_index: current_map_index,
                        first_block: current_map_block_starts
                            .first()
                            .map(|(b, _)| *b)
                            .unwrap_or(0),
                        last_block: *block_number,
                        last_block_hash: block.hash_slow(),
                        block_lv_pointers: std::mem::take(&mut current_map_block_starts),
                    };
                    completed_maps.push((accumulator.finalize(), metadata));
                    accumulator = FilterMapAccumulator::new(params.clone(), current_map_index + 1);
                }
            } else {
                // Add log value to accumulator
                accumulator.add_log_value(log_value.index, log_value.value)?;
                log_value_index = log_value.index + 1;
            }
        }
    }

    // Store all completed maps
    for (filter_map, metadata) in completed_maps {
        storage.store_filter_map_rows(metadata.map_index, filter_map.to_storage_rows())?;
        storage.store_filter_map_last_block(
            metadata.map_index,
            reth_filter_maps::storage::FilterMapLastBlock {
                block_number: metadata.last_block,
                block_hash: metadata.last_block_hash,
            },
        )?;
    }

    // Finalize any remaining map that hasn't been finalized yet
    if !accumulator.current_map.iter().all(|row| row.is_empty()) {
        let metadata = FilterMapMetadata {
            map_index: accumulator.map_index(),
            first_block: current_map_block_starts.first().map(|(b, _)| *b).unwrap_or(0),
            last_block: last_block.unwrap_or(0),
            last_block_hash: Default::default(),
            block_lv_pointers: current_map_block_starts,
        };
        let filter_map = accumulator.finalize();
        storage.store_filter_map_rows(metadata.map_index, filter_map.to_storage_rows())?;
        storage.store_filter_map_last_block(
            metadata.map_index,
            reth_filter_maps::storage::FilterMapLastBlock {
                block_number: metadata.last_block,
                block_hash: metadata.last_block_hash,
            },
        )?;
        println!("Finalized last map with index: {}", metadata.map_index);
    }

    // Update the filter maps range to reflect what we've indexed
    let range_metadata = reth_filter_maps::storage::FilterMapsRange {
        head_indexed: true,
        blocks_first: first_block.unwrap_or(0),
        blocks_after_last: last_block.unwrap_or(0) + 1,
        head_delimiter: log_value_index,
        maps_first: 0,
        maps_after_last: (log_value_index >> params.log_values_per_map) + 1,
        tail_partial_epoch: 0,
        version: 1,
    };
    storage.update_filter_maps_range(range_metadata)?;

    Ok(())
}
