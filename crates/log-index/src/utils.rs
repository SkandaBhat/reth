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

#[inline]
pub fn union_sorted(a: &[u64], b: &[u64]) -> Vec<u64> {
    let mut out = Vec::with_capacity(a.len() + b.len()); // upper bound
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            core::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            core::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            core::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    if i < a.len() {
        out.extend_from_slice(&a[i..]);
    }
    if j < b.len() {
        out.extend_from_slice(&b[j..]);
    }
    out
}

#[inline]
pub fn intersect_sorted(a: &[u64], b: &[u64]) -> Vec<u64> {
    // Capacity heuristic: no more than smaller input
    let mut out = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            core::cmp::Ordering::Less => i += 1,
            core::cmp::Ordering::Greater => j += 1,
            core::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// Shift matches back by `offset` and keep only those whose base is inside `map_index`.
/// Input must be sorted; output remains sorted.
#[inline]
pub fn shift_filter_to_map(
    sorted: &[u64],
    offset: u64,
    map_index: u64,
    log_values_per_map: u64,
) -> Vec<u64> {
    let mut out = Vec::with_capacity(sorted.len());
    let shift_bits = log_values_per_map; // 2^shift_bits = values_per_map
    for &m in sorted {
        if m < offset {
            continue;
        }
        let base = m - offset;
        if (base >> shift_bits) == map_index {
            out.push(base);
        }
    }
    out
}
