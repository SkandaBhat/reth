use alloy_primitives::Log;
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_log_index::FilterMapsQueryProvider;
use reth_provider::ReceiptProvider;

use crate::storage::InMemoryFilterMapsProvider;

impl FilterMapsQueryProvider for InMemoryFilterMapsProvider {
    fn get_log(&self, log_index: u64) -> reth_errors::ProviderResult<Option<Log>> {
        // Find the block containing this log index using the BTreeMap index
        let (block_number, block_start_log_value_index) =
            match self.find_block_for_log_index(log_index) {
                Some((block, start_lv)) => (block, start_lv),
                None => return Ok(None), // No block contains this log index
            };

        // Get receipts for the block
        let receipts = self
            .provider
            .receipts_by_block(BlockHashOrNumber::Number(block_number))
            .unwrap_or_default()
            .unwrap_or_default();

        let mut current_log_value_index = block_start_log_value_index;

        // Iterate through all logs in the block to find the one at log_index
        for receipt in receipts {
            for log in receipt.logs {
                // Each log occupies multiple log value indices:
                // - 1 for the address
                // - 1 for each topic
                let log_value_count = 1 + log.topics().len() as u64;

                // Check if the target log_index falls within this log's range
                if log_index >= current_log_value_index
                    && log_index < current_log_value_index + log_value_count
                {
                    return Ok(Some(log));
                }

                current_log_value_index += log_value_count;
            }
        }

        // Log index not found in this block (shouldn't happen if index is correct)
        Ok(None)
    }
}
