use alloy_primitives::Log;
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_log_index::{query::FilterMapQueryResult, FilterResult};
use reth_provider::ReceiptProvider;
use std::sync::Arc;

use crate::storage::InMemoryFilterMapsProvider;

pub fn fetch_logs_from_index_result(
    provider: Arc<InMemoryFilterMapsProvider>,
    result: FilterMapQueryResult,
) -> FilterResult<Option<Log>> {
    let log_index = result.log_index;
    let block_number = result.block_number;
    let block_start_log_value_index = result.block_start_lv_index;

    // Get receipts for the block
    let receipts = provider
        .provider
        .receipts_by_block(BlockHashOrNumber::Number(block_number))?
        .unwrap_or_default();

    let mut current_log_value_index = block_start_log_value_index;
    // Iterate through all logs in the block to find the one at log_index
    for receipt in receipts {
        for log in receipt.logs {
            // Each log occupies multiple log value indices:
            // - 1 for the address
            // - 1 for each topic
            let log_value_count = 1 + log.topics().len() as u64;

            // check if this is the log we are looking for
            if log_index == current_log_value_index {
                return Ok(Some(log));
            } else if log_index < current_log_value_index {
                return Ok(None);
            }

            // increment the current log value index by the number of log values in this log and
            // check the next log
            current_log_value_index += log_value_count;
        }
    }

    // Log index not found in this block (shouldn't happen if index is correct)
    Ok(None)
}
