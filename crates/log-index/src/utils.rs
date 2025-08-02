use crate::types::{BlockDelimiter, LogValue};
use alloy_primitives::{Address, B256};
use reth_primitives_traits::{AlloyBlockHeader, Block, BlockBody, Receipt, SignedTransaction};
use sha2::{Digest, Sha256};

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

/// Extracts log values from a block.
///
/// This function extracts log values from a block and returns them as a vector of `LogValue`s.
///
/// # Arguments
///
pub fn extract_log_values_from_block<B: Block, R: Receipt>(
    block: B,
    receipts: Vec<R>,
) -> (BlockDelimiter, Vec<LogValue>) {
    let mut log_values: Vec<LogValue> = Vec::new();
    let header = block.header();
    let block_number = header.number();
    let parent_hash = header.parent_hash();
    let parent_timestamp = header.timestamp();
    let transactions = block.body().transactions();

    // Add block delimiter
    let delimiter =
        BlockDelimiter { block_number, block_hash: parent_hash, timestamp: parent_timestamp };

    // Add log values
    for (tx_index, (receipt, transaction)) in receipts.iter().zip(transactions).enumerate() {
        let transaction_hash = *transaction.tx_hash();
        for (log_index, log) in receipt.logs().iter().enumerate() {
            // Address value
            let address_value = address_value(&log.address);
            log_values.push(LogValue {
                value: address_value,
                transaction_hash,
                block_number,
                transaction_index: tx_index as u64,
                log_in_tx_index: log_index as u64,
            });

            // Topic values
            for topic in log.topics() {
                let topic_value = topic_value(&topic);
                log_values.push(LogValue {
                    value: topic_value,
                    transaction_hash,
                    block_number,
                    transaction_index: tx_index as u64,
                    log_in_tx_index: log_index as u64,
                });
            }
        }
    }

    (delimiter, log_values)
}
