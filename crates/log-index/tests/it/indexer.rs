use crate::storage::InMemoryFilterMapsProvider;
use alloy_consensus::TxReceipt;
use alloy_primitives::{map::HashMap, BlockNumber};
use reth_log_index::{
    log_values::{LogValueIterator, RowCell},
    utils::{address_value, count_log_values_in_block, topic_value},
    BlockBoundary, FilterMapParams, FilterMapsReader, FilterMapsWriter, FilterResult,
};
use reth_provider::{test_utils::MockEthProvider, ReceiptProvider};
use std::{ops::RangeInclusive, sync::Arc};
use tracing::{error, info};

// this will be the execute() in the index logs stage
pub(crate) async fn index(
    provider: Arc<MockEthProvider>,
    range: RangeInclusive<BlockNumber>,
    storage: Arc<InMemoryFilterMapsProvider>,
) -> FilterResult<()> {
    let params = FilterMapParams::default();

    // Load / initialize metadata
    let mut md = storage.get_metadata()?.unwrap_or_default();
    if md.first_indexed_block == 0 {
        md.first_indexed_block = *range.start();
    }
    if md.first_map_index == 0 {
        md.first_map_index = (md.next_log_value_index >> params.log_values_per_map) as u32;
    }

    let from_block = *range.start();
    let receipts = provider.receipts_by_block_range(range.clone()).unwrap_or_default();

    let block_boundaries = receipts.clone().into_iter().enumerate().scan(
        md.next_log_value_index,
        |index, (i, receipts)| {
            let boundary =
                BlockBoundary { block_number: from_block + i as u64, log_value_index: *index };
            *index += count_log_values_in_block(&receipts) as u64;
            Some(boundary)
        },
    );

    let projected_log_value_index = block_boundaries
        .clone()
        .last()
        .map(|b| b.log_value_index)
        .unwrap_or(md.next_log_value_index);
    let projected_last_map_index = (projected_log_value_index >> params.log_values_per_map) as u32;
    let cutoff_log_value_index = (projected_last_map_index << params.log_values_per_map) as u64;

    let log_values = receipts.iter().flat_map(|rs| {
        rs.iter().flat_map(|r| {
            r.logs().iter().flat_map(|log| {
                std::iter::once(address_value(&log.address))
                    .chain(log.topics().iter().map(topic_value))
            })
        })
    });

    let mut log_values_iter =
        LogValueIterator::new(log_values, md.next_log_value_index, params.clone());

    let mut block_log_value_indices: Vec<BlockBoundary> = Vec::new();
    {
        for block_boundary in block_boundaries {
            if block_boundary.log_value_index < cutoff_log_value_index {
                info!("Inserting boundary: {:?}", block_boundary);
                block_log_value_indices.push(block_boundary);
            }
        }
    }

    storage.store_log_value_indices_batch(block_log_value_indices.clone())?;

    let mut rows: HashMap<u64, Vec<u32>> = HashMap::default();
    // drain rows until cutoff
    {
        for cell in log_values_iter.by_ref() {
            let RowCell { map, map_row, col, index } = cell?;
            if index < cutoff_log_value_index {
                rows.entry(map_row).or_default().push(col);
            } else {
                break;
            }
        }
    }
    info!("Drained {} rows", rows.len());
    // store rows
    for (map_row_index, row) in rows.clone() {
        storage.store_filter_map_row(map_row_index, row)?;
    }

    // update metadata
    // - last_indexed_block
    // - last_map_index
    // - next_log_value_index
    // - first_map_index
    // - first_indexed_block
    md.last_indexed_block =
        block_log_value_indices.last().map(|b| b.block_number).unwrap_or(md.last_indexed_block);
    md.last_map_index = projected_last_map_index;
    md.next_log_value_index = cutoff_log_value_index + 1;

    storage.store_meta(md)?;

    Ok(())
}
