use alloy_primitives::{Address, B256};
use reth_ethereum_primitives::Receipt;
use sha2::{Digest, Sha256};

use crate::types::LogValue;

/// Compute the log value hash of a log emitting address.
pub fn address_value(address: &Address) -> B256 {
    let mut hasher = Sha256::new();
    hasher.update(address.as_slice());
    B256::from_slice(&hasher.finalize())
}

/// Compute the log value hash of a log topic.
pub fn topic_value(topic: &B256) -> B256 {
    let mut hasher = Sha256::new();
    hasher.update(topic.as_slice());
    B256::from_slice(&hasher.finalize())
}

pub fn extract_log_values_from_block(
    log_value_index: u64,
    block_number: u64,
    block_hash: B256,
    parent_hash: B256,
    receipts: Vec<Receipt>,
) -> Vec<LogValue> {
    let mut log_value_index = log_value_index;
    let mut log_values: Vec<LogValue> = Vec::new();

    // Add block delimiter
    if block_number > 0 {
        let delimiter = LogValue {
            block_number: block_number - 1,
            block_hash: parent_hash,
            index: log_value_index,
            value: B256::ZERO,
            is_block_delimiter: true,
        };
        log_values.push(delimiter);
    }

    // Add log values
    for receipt in receipts {
        for log in receipt.logs {
            // increment log_value_index
            log_value_index += 1;

            // Address value
            let address_value = address_value(&log.address);
            log_values.push(LogValue {
                block_number,
                block_hash,
                index: log_value_index,
                value: address_value,
                is_block_delimiter: false,
            });

            // Topic values
            for topic in log.topics() {
                log_value_index += 1;
                let topic_value = topic_value(&topic);
                log_values.push(LogValue {
                    block_number,
                    block_hash,
                    index: log_value_index,
                    value: topic_value,
                    is_block_delimiter: false,
                });
            }
        }
    }

    log_values
}
