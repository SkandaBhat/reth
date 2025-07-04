//! Log value calculation for FilterMaps based on EIP-7745.

use alloy_primitives::{Address, B256};
use alloy_rpc_types_eth::Log;
use sha2::{Digest, Sha256};

/// A log value with its global index.
// #[derive(Debug, Clone, PartialEq, Eq)]
// pub struct LogValue {
//     /// Global log value index
//     pub index: u64,
//     /// The computed hash value
//     pub value: B256,
// }

/// Process a log entry to extract all log values.
///
/// Returns log values for:
/// - The contract address
/// - Each indexed topic (up to 4)
///
/// The `lv_index` is updated to track the global log value index.
/// A delimiter (empty value) is added before the first log of each block.
// pub fn process_log(log: &Log, lv_index: &mut u64, is_first_in_block: bool) -> Vec<LogValue> {
//     let mut values = Vec::new();

//     // Add delimiter for new block
//     if is_first_in_block {
//         *lv_index += 1;
//     }

//     // Add address value
//     values.push(LogValue { index: *lv_index, value: address_value(&log.address()) });
//     *lv_index += 1;

//     // Add topic values
//     for topic in log.topics() {
//         values.push(LogValue { index: *lv_index, value: topic_value(topic) });
//         *lv_index += 1;
//     }

//     values
// }

#[cfg(test)]
mod tests {

    // #[test]
    // fn test_process_log() {
    //     let log = Log {
    //         inner: alloy_primitives::Log::new(
    //             address!("0000000000000000000000000000000000000001"),
    //             vec![
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000001"),
    //                 b256!("0000000000000000000000000000000000000000000000000000000000000002"),
    //             ],
    //             vec![].into(),
    //         )
    //         .unwrap(),
    //         block_hash: None,
    //         block_number: None,
    //         transaction_hash: None,
    //         transaction_index: None,
    //         log_index: None,
    //         removed: false,
    //         block_timestamp: None,
    //     };

    //     let mut lv_index = 100;
    //     let values = process_log(&log, &mut lv_index, false);

    //     // Should have 3 values: address + 2 topics
    //     assert_eq!(values.len(), 3);
    //     assert_eq!(values[0].index, 100);
    //     assert_eq!(values[1].index, 101);
    //     assert_eq!(values[2].index, 102);
    //     assert_eq!(lv_index, 103);

    //     // First log in block should increment index by 1 for delimiter
    //     let mut lv_index2 = 200;
    //     let values2 = process_log(&log, &mut lv_index2, true);
    //     assert_eq!(values2[0].index, 201); // Skipped 200 for delimiter
    //     assert_eq!(lv_index2, 204);
    // }
}
