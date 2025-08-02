//! Stage for indexing logs.
//!
//! This stage processes blocks and their logs to build filter maps according to EIP-7745.

use reth_ethereum_primitives::{Block, Receipt};
use reth_log_index::{
    extract_log_values_from_block, FilterMapAccumulator, FilterMapParams, FilterMapsReader,
    FilterMapsWriter,
};
use reth_provider::{BlockReader, DBProvider, ReceiptProvider};
use reth_stages_api::{
    ExecInput, ExecOutput, Stage, StageCheckpoint, StageError, StageId, UnwindInput, UnwindOutput,
};
use std::{fmt::Debug, sync::Arc};
use tracing::info;

/// The logs indexing stage.
///
/// This stage processes blocks and their logs to build filter maps according to EIP-7745.
#[derive(Debug, Clone)]
pub struct IndexLogsStage {
    /// The filter map parameters.
    params: Arc<FilterMapParams>,
    /// commit threshold
    commit_threshold: u64,
}

impl IndexLogsStage {
    /// Creates a new filter maps indexing stage with the given parameters.
    pub fn new(params: Arc<FilterMapParams>, commit_threshold: u64) -> Self {
        Self { params, commit_threshold }
    }
}

impl Default for IndexLogsStage {
    fn default() -> Self {
        Self::new(Arc::new(FilterMapParams::default()), 5_000_000)
    }
}

impl<Provider> Stage<Provider> for IndexLogsStage
where
    Provider: DBProvider
        + FilterMapsReader
        + FilterMapsWriter
        + BlockReader<Block = Block>
        + ReceiptProvider<Receipt = Receipt>,
{
    /// Return the id of the stage.
    fn id(&self) -> StageId {
        StageId::Other("IndexLogs")
    }

    /// Execute the stage.
    fn execute(
        &mut self,
        provider: &Provider,
        mut input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        let target_block = input.target();
        let checkpoint = input.checkpoint();
        let start_block = checkpoint.block_number + 1;

        if start_block > target_block {
            return Ok(ExecOutput::done(StageCheckpoint::new(target_block)));
        }

        info!(
            target: "sync::stages::index_logs",
            "Executing logs indexing stage from {} to {}",
            start_block,
            target_block
        );

        let metadata = provider
            .get_metadata()
            .map_err(|e| StageError::Fatal(Box::new(e)))?
            .unwrap_or_default();

        let mut accumulator = FilterMapAccumulator::new((*self.params).clone(), metadata);

        loop {
            let (tx_range, block_range, is_final_range) = input
                .next_block_range_with_transaction_threshold(provider, self.commit_threshold)?;

            info!(
                target: "sync::stages::index_logs",
                ?tx_range,
                "Processing block range"
            );

            let blocks = provider.block_range(block_range.clone()).unwrap_or_default();
            let receipts =
                provider.receipts_by_block_range(block_range.clone()).unwrap_or_default();

            for (block, block_receipts) in blocks.into_iter().zip(receipts) {
                let (block_delimiter, log_values) =
                    extract_log_values_from_block(block, block_receipts);
                accumulator
                    .process_block(block_delimiter, log_values)
                    .map_err(|e| StageError::Fatal(Box::new(e)))?;
            }

            info!(
                target: "sync::stages::index_logs",
                ?tx_range,
                "Processed block range"
            );

            input.checkpoint = Some(StageCheckpoint::new(*block_range.end()));

            if is_final_range {
                // store completed maps, along with block log value indices and metadata
                for completed_map in accumulator.drain_completed_maps() {
                    provider
                        .store_filter_map_rows(completed_map.rows().clone())
                        .map_err(|e| StageError::Fatal(Box::new(e)))?;

                    for (block_number, log_value_index) in completed_map.block_log_value_indices() {
                        provider
                            .store_log_value_index_for_block(*block_number, *log_value_index)
                            .map_err(|e| StageError::Fatal(Box::new(e)))?;
                    }
                }
                info!(
                    target: "sync::stages::index_logs",
                    "Storing completed maps"
                );

                provider
                    .store_metadata(accumulator.metadata.clone())
                    .map_err(|e| StageError::Fatal(Box::new(e)))?;

                info!(
                    target: "sync::stages::index_logs",
                    "Done processing block range"
                );

                break;
            }
        }

        Ok(ExecOutput::done(StageCheckpoint::new(input.target())))
    }

    /// Unwind the stage.
    fn unwind(
        &mut self,
        _provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let unwind_to = input.unwind_to;

        // TODO: Implement
        // 1. Remove filter map data for blocks > unwind_to
        // 2. Update filter maps metadata
        // 3. Restore lv_index to the correct position

        Ok(UnwindOutput { checkpoint: StageCheckpoint::new(unwind_to) })
    }
}
