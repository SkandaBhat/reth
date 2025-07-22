// Test implementation of FilterMapsReader and FilterMapsWriter traits

use alloy_primitives::{BlockNumber, Log};
use alloy_rpc_types_eth::BlockHashOrNumber;
use reth_filter_maps::storage::{
    FilterMapLastBlock, FilterMapRow, FilterMapsRange, FilterMapsReader, FilterMapsWriter,
};
use reth_filter_maps::FilterResult;
use reth_provider::test_utils::MockEthProvider;
use reth_provider::ReceiptProvider;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// In-memory implementation of FilterMapsReader and FilterMapsWriter for testing
#[derive(Clone)]
pub(crate) struct InMemoryFilterMapsProvider {
    params: reth_filter_maps::FilterMapParams,
    filter_rows: Arc<Mutex<HashMap<(u32, u32), FilterMapRow>>>,
    block_pointers: Arc<Mutex<HashMap<BlockNumber, u64>>>,
    last_blocks: Arc<Mutex<HashMap<u32, FilterMapLastBlock>>>,
    range: Arc<Mutex<Option<FilterMapsRange>>>,
    provider: Arc<MockEthProvider>,
}

impl InMemoryFilterMapsProvider {
    /// Create a new empty provider
    pub(crate) fn new(provider: Arc<MockEthProvider>) -> Self {
        Self {
            params: reth_filter_maps::FilterMapParams::default(),
            filter_rows: Arc::new(Mutex::new(HashMap::new())),
            block_pointers: Arc::new(Mutex::new(HashMap::new())),
            last_blocks: Arc::new(Mutex::new(HashMap::new())),
            range: Arc::new(Mutex::new(None)),
            provider,
        }
    }
}

impl FilterMapsReader for InMemoryFilterMapsProvider {
    fn get_filter_map_rows(
        &self,
        map_index: u32,
        row_indices: &[u32],
    ) -> FilterResult<Vec<FilterMapRow>> {
        let rows = self.filter_rows.lock().unwrap();
        let mut result = Vec::new();

        for &row_idx in row_indices {
            if let Some(row) = rows.get(&(map_index, row_idx)) {
                result.push(row.clone());
            }
        }

        Ok(result)
    }

    fn get_block_lv_pointer(&self, block: BlockNumber) -> FilterResult<Option<u64>> {
        let pointer = self.block_pointers.lock().unwrap().get(&block).copied();
        Ok(pointer)
    }

    fn get_filter_map_last_block(
        &self,
        map_index: u32,
    ) -> FilterResult<Option<FilterMapLastBlock>> {
        Ok(self.last_blocks.lock().unwrap().get(&map_index).cloned())
    }

    fn get_filter_maps_range(&self) -> FilterResult<Option<FilterMapsRange>> {
        Ok(self.range.lock().unwrap().clone())
    }
}

impl FilterMapsWriter for InMemoryFilterMapsProvider {
    fn store_filter_map_rows(
        &self,
        map_index: u32,
        rows: Vec<(u32, FilterMapRow)>,
    ) -> FilterResult<()> {
        let mut filter_rows = self.filter_rows.lock().unwrap();
        for (row_idx, row) in rows {
            filter_rows.insert((map_index, row_idx), row);
        }
        Ok(())
    }

    fn store_block_lv_pointer(&self, block: BlockNumber, lv_pointer: u64) -> FilterResult<()> {
        self.block_pointers.lock().unwrap().insert(block, lv_pointer);
        Ok(())
    }

    fn store_filter_map_last_block(
        &self,
        map_index: u32,
        last_block: FilterMapLastBlock,
    ) -> FilterResult<()> {
        self.last_blocks.lock().unwrap().insert(map_index, last_block);
        Ok(())
    }

    fn update_filter_maps_range(&self, range: FilterMapsRange) -> FilterResult<()> {
        *self.range.lock().unwrap() = Some(range);
        Ok(())
    }
}

impl reth_filter_maps::FilterMapProvider for InMemoryFilterMapsProvider {
    fn params(&self) -> &reth_filter_maps::FilterMapParams {
        &self.params
    }

    fn block_to_log_index(&self, block_number: BlockNumber) -> reth_errors::ProviderResult<u64> {
        // First try to get exact match without holding lock
        if let Ok(Some(pointer)) = self.get_block_lv_pointer(block_number) {
            return Ok(pointer);
        }

        // Check the filter maps range to see if this block is within indexed range
        if let Ok(Some(range)) = self.get_filter_maps_range() {
            if block_number < range.blocks_first {
                // Block is before indexed range
                return Ok(0);
            } else if block_number >= range.blocks_after_last {
                // Block is after indexed range, return the head delimiter (last indexed position)
                return Ok(range.head_delimiter);
            }
        }

        // Block should be within range but doesn't have exact pointer, find nearest
        let next_lv_index = {
            let block_pointers = self.block_pointers.lock().unwrap();
            if block_pointers.is_empty() {
                // No blocks indexed yet
                return Ok(0);
            }

            let mut blocks: Vec<_> = block_pointers.iter().map(|(&k, &v)| (k, v)).collect();
            blocks.sort_by_key(|(block, _)| *block);

            // Find the first block >= block_number
            blocks
                .iter()
                .find(|(block, _)| *block >= block_number)
                .map(|(_, lv_index)| *lv_index)
                .unwrap_or(0)
        }; // Lock is released here

        Ok(next_lv_index)
    }

    fn get_filter_rows(
        &self,
        map_indices: &[u32],
        row_index: u32,
        _layer: u32,
    ) -> reth_errors::ProviderResult<Vec<reth_filter_maps::FilterRow>> {
        let mut result = Vec::new();
        for &map_index in map_indices {
            if let Ok(rows) = self.get_filter_map_rows(map_index, &[row_index]) {
                if let Some(row) = rows.first() {
                    result.push(row.columns.clone());
                } else {
                    // Push empty row if not found
                    result.push(Vec::new());
                }
            } else {
                // Push empty row on error
                result.push(Vec::new());
            }
        }
        Ok(result)
    }

    fn get_log(&self, log_index: u64) -> reth_errors::ProviderResult<Option<Log>> {
        // Binary search to find the block containing this log_index
        let (block_number, block_start_lv) = {
            let block_pointers = self.block_pointers.lock().unwrap();
            let mut blocks: Vec<_> = block_pointers.iter().collect();
            blocks.sort_by_key(|(block, _)| **block);

            let block_number = if blocks.is_empty() {
                None
            } else {
                // Find the block that contains this log_index
                // We need to find the last block whose lv_index <= log_index
                let mut result_block = None;

                for i in 0..blocks.len() {
                    let (block, lv) = blocks[i];

                    // Skip blocks that start after our log_index
                    if *lv > log_index {
                        break;
                    }

                    // Check if this block contains the log
                    if i + 1 < blocks.len() {
                        let (_, next_lv) = blocks[i + 1];
                        if *lv <= log_index && log_index < *next_lv {
                            result_block = Some(*block);
                            break;
                        }
                    } else {
                        // This is the last block, check if log is in it
                        result_block = Some(*block);
                    }
                }

                result_block
            };

            let block_number = block_number.unwrap();
            let block_start_lv = block_pointers.get(&block_number).copied();

            (block_number, block_start_lv)
        }; // Lock is released here

        let receipts = self
            .provider
            .receipts_by_block(BlockHashOrNumber::Number(block_number))
            .unwrap_or_default()
            .unwrap_or_default();

        let mut current_lv = block_start_lv.unwrap_or(0);

        // skip delimiter
        current_lv += 1;

        for receipt in receipts {
            for log in receipt.logs {
                // check if this is the target log
                let log_start = current_lv;
                let log_end = current_lv + 1 + log.topics().len() as u64;

                if log_index >= log_start && log_index < log_end {
                    return Ok(Some(log));
                }

                current_lv = log_end;
            }
        }
        Ok(None)
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use alloy_primitives::B256;

//     #[test]
//     fn test_reader_writer_integration() {
//         let provider = InMemoryFilterMapsProvider::new();

//         // Store some filter map rows
//         let rows =
//             vec![(0, FilterMapRow::new(vec![10, 20, 30])), (5, FilterMapRow::new(vec![40, 50]))];
//         provider.store_filter_map_rows(1, rows).unwrap();

//         // Read them back
//         let retrieved = provider.get_filter_map_rows(1, &[0, 5, 10]).unwrap();
//         assert_eq!(retrieved.len(), 2);
//         assert_eq!(retrieved[0].columns, vec![10, 20, 30]);
//         assert_eq!(retrieved[1].columns, vec![40, 50]);

//         // Store and read block pointer
//         provider.store_block_lv_pointer(100, 5000).unwrap();
//         let pointer = provider.get_block_lv_pointer(100).unwrap();
//         assert_eq!(pointer, Some(5000));

//         // Non-existent block
//         let no_pointer = provider.get_block_lv_pointer(200).unwrap();
//         assert_eq!(no_pointer, None);
//     }

//     #[test]
//     fn test_last_block_storage() {
//         let provider = InMemoryFilterMapsProvider::new();

//         let last_block1 =
//             FilterMapLastBlock { block_number: 1000, block_hash: B256::repeat_byte(0xAA) };

//         let last_block2 =
//             FilterMapLastBlock { block_number: 2000, block_hash: B256::repeat_byte(0xBB) };

//         // Store last blocks for different maps
//         provider.store_filter_map_last_block(0, last_block1.clone()).unwrap();
//         provider.store_filter_map_last_block(1, last_block2.clone()).unwrap();

//         // Read them back
//         let retrieved1 = provider.get_filter_map_last_block(0).unwrap().unwrap();
//         assert_eq!(retrieved1.block_number, 1000);
//         assert_eq!(retrieved1.block_hash, B256::repeat_byte(0xAA));

//         let retrieved2 = provider.get_filter_map_last_block(1).unwrap().unwrap();
//         assert_eq!(retrieved2.block_number, 2000);
//         assert_eq!(retrieved2.block_hash, B256::repeat_byte(0xBB));

//         // Non-existent map
//         let no_block = provider.get_filter_map_last_block(5).unwrap();
//         assert!(no_block.is_none());
//     }

//     #[test]
//     fn test_range_storage() {
//         let provider = InMemoryFilterMapsProvider::new();

//         // Initially no range
//         let no_range = provider.get_filter_maps_range().unwrap();
//         assert!(no_range.is_none());

//         // Store a range
//         let range = FilterMapsRange {
//             head_indexed: true,
//             blocks_first: 100,
//             blocks_after_last: 5000,
//             head_delimiter: 250000,
//             maps_first: 0,
//             maps_after_last: 5,
//             tail_partial_epoch: 1,
//             version: 1,
//         };

//         provider.update_filter_maps_range(range.clone()).unwrap();

//         // Read it back
//         let retrieved = provider.get_filter_maps_range().unwrap().unwrap();
//         assert_eq!(retrieved.head_indexed, true);
//         assert_eq!(retrieved.blocks_first, 100);
//         assert_eq!(retrieved.blocks_after_last, 5000);
//         assert_eq!(retrieved.head_delimiter, 250000);
//         assert_eq!(retrieved.maps_first, 0);
//         assert_eq!(retrieved.maps_after_last, 5);
//         assert_eq!(retrieved.tail_partial_epoch, 1);
//         assert_eq!(retrieved.version, 1);
//     }
// }
