//! MapRenderer for building filter maps from blockchain logs.

use alloy_consensus::TxReceipt;
use alloy_primitives::BlockNumber;
use reth_storage_api::FilterMapWriter;
use roaring::RoaringBitmap;
use std::collections::HashMap;

use super::{
    column_index, map_index_from_lv_index, map_row_index, row_index, FilterMapParams, LogValue,
};

/// Errors that can occur during filter map rendering.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// Database error
    #[error("database error: {0}")]
    Database(String),
    /// Invalid log data
    #[error("invalid log data: {0}")]
    InvalidLog(String),
}

/// Statistics for a rendering operation.
#[derive(Debug, Default)]
pub struct RenderStats {
    /// Number of blocks processed
    pub blocks_processed: u64,
    /// Number of logs processed
    pub logs_processed: u64,
    /// Number of log values indexed
    pub log_values_indexed: u64,
    /// Number of filter rows written
    pub rows_written: u64,
    /// Number of maps completed
    pub maps_completed: u32,
}

/// Accumulator for building filter map rows.
#[derive(Debug)]
struct RowAccumulator {
    /// Map index -> Row index -> Bitmap of columns
    rows: HashMap<u32, HashMap<u32, RoaringBitmap>>,
    /// Current statistics
    stats: RenderStats,
}

impl RowAccumulator {
    fn new() -> Self {
        Self { rows: HashMap::new(), stats: RenderStats::default() }
    }

    /// Add log values to the accumulator.
    fn add_log_values(&mut self, lv_index: u64, values: &[LogValue], params: &FilterMapParams) {
        // Calculate the map index for this log value
        let map_index = map_index_from_lv_index(lv_index, params);

        // Add each value (address + topics) to the appropriate rows
        for (i, value) in values.iter().enumerate() {
            let current_lv_index = lv_index + i as u64;

            // Add to all three layers
            for layer in 0..3 {
                let row_idx = row_index(map_index, layer, &value.value, params);
                let col_idx = column_index(current_lv_index, &value.value, params);

                // Get or create the row bitmap
                self.rows
                    .entry(map_index)
                    .or_insert_with(HashMap::new)
                    .entry(row_idx)
                    .or_insert_with(RoaringBitmap::new)
                    .insert(col_idx);
            }
        }

        self.stats.log_values_indexed += values.len() as u64;
    }

    /// Flush accumulated rows to storage.
    async fn flush_rows<W: FilterMapWriter>(
        &mut self,
        writer: &W,
        params: &FilterMapParams,
    ) -> Result<(), RenderError> {
        for (map_index, map_rows) in self.rows.drain() {
            for (row_index, bitmap) in map_rows {
                // Calculate global row index
                let global_row_idx = map_row_index(map_index, row_index, params);

                // Serialize the bitmap
                let mut row_data = Vec::new();
                bitmap.serialize_into(&mut row_data).map_err(|e| {
                    RenderError::Database(format!("Failed to serialize bitmap: {}", e))
                })?;

                // Write to storage
                writer
                    .put_filter_map_row(global_row_idx, row_data)
                    .map_err(|e| RenderError::Database(e.to_string()))?;

                self.stats.rows_written += 1;
            }
        }

        Ok(())
    }
}

/// MapRenderer builds filter maps from blockchain logs.
#[derive(Debug)]
pub struct MapRenderer<W: FilterMapWriter> {
    writer: W,
    params: FilterMapParams,
    /// Current log value index
    current_lv_index: u64,
    /// Accumulator for building rows
    accumulator: RowAccumulator,
    /// Map boundaries tracking
    map_boundaries: HashMap<u32, (BlockNumber, u64)>,
}

impl<W: FilterMapWriter> MapRenderer<W> {
    /// Create a new MapRenderer.
    pub fn new(writer: W, params: FilterMapParams, starting_lv_index: u64) -> Self {
        Self {
            writer,
            params,
            current_lv_index: starting_lv_index,
            accumulator: RowAccumulator::new(),
            map_boundaries: HashMap::new(),
        }
    }

    /// Process a block's receipts and add logs to filter maps.
    pub async fn process_block<R>(
        &mut self,
        block_number: BlockNumber,
        receipts: &[R],
    ) -> Result<(), RenderError>
    where
        R: TxReceipt<Log = alloy_primitives::Log>,
    {
        let block_start_lv_index = self.current_lv_index;
        let mut is_first_in_block = true;

        // Process all logs in the block
        for receipt in receipts {
            for log in receipt.logs() {
                // Convert to RPC log type and process
                let rpc_log = alloy_rpc_types_eth::Log {
                    inner: log.clone(),
                    block_hash: None,
                    block_number: Some(block_number),
                    block_timestamp: None,
                    transaction_hash: None,
                    transaction_index: None,
                    log_index: None,
                    removed: false,
                };

                let values =
                    super::process_log(&rpc_log, &mut self.current_lv_index, is_first_in_block);
                is_first_in_block = false;

                self.accumulator.add_log_values(
                    self.current_lv_index - values.len() as u64,
                    &values,
                    &self.params,
                );
                self.accumulator.stats.logs_processed += 1;
            }
        }

        // Update block log range if any logs were processed
        if self.current_lv_index > block_start_lv_index {
            self.writer
                .put_block_log_range(block_number, block_start_lv_index, self.current_lv_index - 1)
                .map_err(|e| RenderError::Database(e.to_string()))?;

            // Also store the pointer for efficient searching
            self.writer
                .put_block_log_value_pointer(block_number, block_start_lv_index)
                .map_err(|e| RenderError::Database(e.to_string()))?;
        }

        // Track map boundaries
        if self.current_lv_index > 0 {
            let current_map = map_index_from_lv_index(self.current_lv_index - 1, &self.params);
            self.map_boundaries.insert(current_map, (block_number, self.current_lv_index - 1));
        }

        self.accumulator.stats.blocks_processed += 1;

        // Check if we should flush (e.g., every 1000 blocks or at map boundaries)
        if self.should_flush(block_number) {
            self.flush().await?;
        }

        Ok(())
    }

    /// Check if we should flush accumulated data.
    fn should_flush(&self, block_number: BlockNumber) -> bool {
        // Flush every 1000 blocks or when crossing map boundaries
        block_number % 1000 == 0 ||
            self.accumulator.rows.len() > 100 || // Too many maps in memory
            self.has_completed_maps()
    }

    /// Check if we have any completed maps that can be finalized.
    fn has_completed_maps(&self) -> bool {
        if self.current_lv_index == 0 {
            return false;
        }

        let current_map = map_index_from_lv_index(self.current_lv_index - 1, &self.params);
        let starting_map = if self.accumulator.rows.is_empty() {
            current_map
        } else {
            *self.accumulator.rows.keys().min().unwrap()
        };

        // We have completed maps if current map is beyond any accumulated map
        current_map > starting_map
    }

    /// Flush accumulated data to storage.
    pub async fn flush(&mut self) -> Result<(), RenderError> {
        // Flush row data
        self.accumulator.flush_rows(&self.writer, &self.params).await?;

        // Update map boundaries
        for (map_index, (last_block, last_lv_index)) in self.map_boundaries.drain() {
            self.writer
                .put_map_boundary(map_index, last_block, last_lv_index)
                .map_err(|e| RenderError::Database(e.to_string()))?;

            self.accumulator.stats.maps_completed += 1;
        }

        Ok(())
    }

    /// Finalize rendering and update metadata.
    pub async fn finalize(mut self, final_block: BlockNumber) -> Result<RenderStats, RenderError> {
        // Flush any remaining data
        self.flush().await?;

        // Update global metadata
        let total_maps = if self.current_lv_index == 0 {
            0
        } else {
            map_index_from_lv_index(self.current_lv_index - 1, &self.params) + 1
        };

        self.writer
            .put_filter_map_metadata(final_block, self.current_lv_index, total_maps)
            .map_err(|e| RenderError::Database(e.to_string()))?;

        Ok(self.accumulator.stats)
    }

    /// Get current statistics.
    pub fn stats(&self) -> &RenderStats {
        &self.accumulator.stats
    }

    /// Get current log value index.
    pub fn current_log_value_index(&self) -> u64 {
        self.current_lv_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256, Bloom, Log, LogData};
    use alloy_consensus::Eip658Value;
    use reth_errors::ProviderResult;
    use std::collections::HashMap;

    /// Mock writer for testing
    #[allow(dead_code)]
    struct MockFilterMapWriter {
        rows: HashMap<u64, Vec<u8>>,
        block_ranges: HashMap<BlockNumber, (u64, u64)>,
        block_pointers: HashMap<BlockNumber, u64>,
        map_boundaries: HashMap<u32, (BlockNumber, u64)>,
        metadata: Option<(BlockNumber, u64, u32)>,
    }

    impl MockFilterMapWriter {
        fn new() -> Self {
            Self {
                rows: HashMap::new(),
                block_ranges: HashMap::new(),
                block_pointers: HashMap::new(),
                map_boundaries: HashMap::new(),
                metadata: None,
            }
        }
    }

    impl FilterMapWriter for MockFilterMapWriter {
        fn put_filter_map_row(
            &self,
            _global_row_index: u64,
            _row_data: Vec<u8>,
        ) -> ProviderResult<()> {
            // In a real implementation, this would write to database
            // For testing, we'll just store in memory
            Ok(())
        }

        fn put_map_boundary(
            &self,
            _map_index: u32,
            _last_block: BlockNumber,
            _last_log_value_index: u64,
        ) -> ProviderResult<()> {
            Ok(())
        }

        fn put_block_log_range(
            &self,
            _block_number: BlockNumber,
            _start_index: u64,
            _end_index: u64,
        ) -> ProviderResult<()> {
            Ok(())
        }

        fn put_block_log_value_pointer(
            &self,
            _block_number: BlockNumber,
            _start_index: u64,
        ) -> ProviderResult<()> {
            Ok(())
        }

        fn put_filter_map_metadata(
            &self,
            _indexed_height: BlockNumber,
            _total_log_values: u64,
            _total_maps: u32,
        ) -> ProviderResult<()> {
            Ok(())
        }

        fn delete_filter_maps(
            &self,
            _block_range: impl std::ops::RangeBounds<BlockNumber>,
        ) -> ProviderResult<u64> {
            Ok(0)
        }
    }

    /// Mock receipt for testing
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct MockReceipt {
        logs: Vec<Log>,
    }

    impl TxReceipt for MockReceipt {
        type Log = Log;

        fn status_or_post_state(&self) -> Eip658Value {
            Eip658Value::Eip658(true)
        }

        fn status(&self) -> bool {
            true
        }

        fn bloom(&self) -> Bloom {
            Bloom::default()
        }

        fn cumulative_gas_used(&self) -> u64 {
            21000
        }

        fn logs(&self) -> &[Self::Log] {
            &self.logs
        }
    }

    #[tokio::test]
    async fn test_map_renderer_basic() {
        let writer = MockFilterMapWriter::new();
        let params = FilterMapParams::default();
        let mut renderer = MapRenderer::new(writer, params, 0);

        // Create a test log
        let log = Log {
            address: address!("0000000000000000000000000000000000000001"),
            data: LogData::new_unchecked(
                vec![b256!("0000000000000000000000000000000000000000000000000000000000000001")],
                Default::default(),
            ),
        };

        let receipt = MockReceipt { logs: vec![log] };

        // Process a block
        renderer.process_block(100, &[receipt]).await.unwrap();

        // Check stats
        assert_eq!(renderer.stats().blocks_processed, 1);
        assert_eq!(renderer.stats().logs_processed, 1);
        assert_eq!(renderer.stats().log_values_indexed, 2); // 1 address + 1 topic

        // Finalize
        let stats = renderer.finalize(100).await.unwrap();
        assert_eq!(stats.blocks_processed, 1);
    }

    #[tokio::test]
    async fn test_map_renderer_multiple_logs() {
        let writer = MockFilterMapWriter::new();
        let params = FilterMapParams::default();
        let mut renderer = MapRenderer::new(writer, params, 0);

        // Create multiple logs with different numbers of topics
        let logs = vec![
            Log {
                address: address!("0000000000000000000000000000000000000001"),
                data: LogData::new_unchecked(
                    vec![
                        b256!("0000000000000000000000000000000000000000000000000000000000000001"),
                        b256!("0000000000000000000000000000000000000000000000000000000000000002"),
                    ],
                    Default::default(),
                ),
            },
            Log {
                address: address!("0000000000000000000000000000000000000002"),
                data: LogData::new_unchecked(
                    vec![b256!("0000000000000000000000000000000000000000000000000000000000000003")],
                    Default::default(),
                ),
            },
        ];

        let receipt = MockReceipt { logs };

        // Process the block
        renderer.process_block(200, &[receipt]).await.unwrap();

        // Check stats
        assert_eq!(renderer.stats().logs_processed, 2);
        assert_eq!(renderer.stats().log_values_indexed, 5); // (1 + 2) + (1 + 1)
        assert_eq!(renderer.current_log_value_index(), 6); // 1 delimiter + 5 log values
    }

    #[test]
    fn test_row_accumulator() {
        let mut acc = RowAccumulator::new();
        let params = FilterMapParams::default();

        // Create test log values
        let values = vec![LogValue {
            index: 0,
            value: crate::filter_maps::address_value(&address!("0000000000000000000000000000000000000001")),
        }];

        // Add the log values
        acc.add_log_values(0, &values, &params);

        // Check that we have entries in the accumulator
        assert!(!acc.rows.is_empty());
        assert_eq!(acc.stats.log_values_indexed, 1); // Just the address

        // Should have entries for map 0
        assert!(acc.rows.contains_key(&0));

        // Should have 3 layers worth of rows
        let map_rows = &acc.rows[&0];
        // Each layer might map to different rows, so we check we have at least one
        assert!(!map_rows.is_empty());
    }

    #[test]
    fn test_map_boundary_tracking() {
        let writer = MockFilterMapWriter::new();
        let params = FilterMapParams::default();
        let renderer = MapRenderer::new(writer, params, 0);

        // Check initial state
        assert_eq!(renderer.current_log_value_index(), 0);
        assert!(renderer.map_boundaries.is_empty());
    }
}
