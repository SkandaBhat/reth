//! Stage for indexing logs.
//!
//! This stage processes blocks and their logs to build filter maps according to EIP-7745.

use reth_config::config::IndexLogsConfig;
use reth_db::{tables, DbTxUnwindExt};
use reth_db_api::transaction::DbTxMut;
use reth_log_index::{
    extract_log_values_from_block, FilterMapAccumulator, FilterMapMeta, FilterMapParams,
    FilterMapsReader, FilterMapsWriter,
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
    pub fn new(config: IndexLogsConfig, params: Arc<FilterMapParams>) -> Self {
        Self { params, commit_threshold: config.commit_threshold }
    }
}

impl Default for IndexLogsStage {
    fn default() -> Self {
        Self::new(IndexLogsConfig::default(), Arc::new(FilterMapParams::default()))
    }
}

impl<Provider> Stage<Provider> for IndexLogsStage
where
    Provider: DBProvider<Tx: DbTxMut>
        + FilterMapsReader
        + FilterMapsWriter
        + BlockReader
        + ReceiptProvider,
{
    /// Return the id of the stage.
    fn id(&self) -> StageId {
        StageId::IndexLogs
    }

    /// Execute the stage.
    fn execute(
        &mut self,
        provider: &Provider,
        mut input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        // TODO: add prune checkpoint
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

        let mut metadata = provider
            .get_metadata()
            .map_err(|e| StageError::Fatal(Box::new(e)))?
            .unwrap_or_default();

        // if we have never indexed logs, there's a chance that the metadata is not set
        // so we need to set it to the first block
        if metadata.first_indexed_block == 0 {
            metadata.first_indexed_block = start_block;
            provider.store_metadata(metadata).map_err(|e| StageError::Fatal(Box::new(e)))?;
        }

        let mut accumulator = FilterMapAccumulator::new((*self.params).clone(), metadata.clone());

        loop {
            let (block_range, is_final_range) =
                input.next_block_range_with_threshold(self.commit_threshold);

            info!(
                target: "sync::stages::index_logs",
                ?block_range,
                "Processing block range"
            );

            let blocks = provider.block_range(block_range.clone()).unwrap_or_default();
            let receipts =
                provider.receipts_by_block_range(block_range.clone()).unwrap_or_default();

            let mut log_values_count = 0;
            for (block, block_receipts) in blocks.into_iter().zip(receipts) {
                let (block_delimiter, log_values) =
                    extract_log_values_from_block(block, block_receipts);

                log_values_count += log_values.len() + 1;

                accumulator
                    .add_block(block_delimiter, log_values)
                    .map_err(|e| StageError::Fatal(Box::new(e)))?;
            }
            // store completed maps, along with block log value indices and metadata
            for completed_map in accumulator.drain_completed_maps() {
                // store filter map rows
                for (map_row_index, row) in completed_map.rows {
                    provider
                        .store_filter_map_row(map_row_index, row)
                        .map_err(|e| StageError::Fatal(Box::new(e)))?;
                }

                // store block log value indices
                for (block_number, log_value_index) in completed_map.block_log_value_indices {
                    provider
                        .store_log_value_index_for_block(block_number, log_value_index)
                        .map_err(|e| StageError::Fatal(Box::new(e)))?;
                }

                // store metadata
                provider
                    .store_metadata(accumulator.metadata.clone())
                    .map_err(|e| StageError::Fatal(Box::new(e)))?;
            }

            info!(
                target: "sync::stages::index_logs",
                "Processed {} log values",
                log_values_count
            );

            info!(
                target: "sync::stages::index_logs",
                ?block_range,
                "Processed block range"
            );

            input.checkpoint = Some(StageCheckpoint::new(*block_range.end()));

            if is_final_range {
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
        provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let unwind_to = input.unwind_to;

        info!(
            target: "sync::stages::index_logs",
            "Unwinding logs indexing stage to {}",
            unwind_to
        );

        // get the metadata
        let metadata = provider
            .get_metadata()
            .map_err(|e| StageError::Fatal(Box::new(e)))?
            .unwrap_or_default();

        // if we havent indexed past the unwind_to block, we can just return the checkpoint
        if metadata.last_indexed_block <= unwind_to {
            return Ok(UnwindOutput { checkpoint: StageCheckpoint::new(unwind_to) });
        }

        // unwind BlockLogIndices table for all blocks > unwind_to
        provider.tx_ref().unwind_table_by_num::<tables::BlockLogIndices>(unwind_to)?;

        // find the log value index for the unwind_to block
        let unwind_log_value_index = provider
            .get_log_value_index_for_block(unwind_to)
            .map_err(|e| StageError::Fatal(Box::new(e)))?
            .unwrap_or(0);

        let unwind_map_index = unwind_log_value_index >> self.params.log_values_per_map;

        let maps_to_keep = unwind_map_index.saturating_sub(1);

        // get the first log value index for the map at unwind_map_index
        let map_first_log_value_index = unwind_map_index << self.params.log_values_per_map;
        let (map_first_block_number, _) = provider
            .find_block_for_log_value_index(metadata, map_first_log_value_index, None)
            .map_err(|e| StageError::Fatal(Box::new(e)))?
            .unwrap_or((0, 0));

        let last_indexed_block = map_first_block_number.saturating_sub(1);

        // TODO: uncomment this when we have a way to delete rows from the database
        // // Delete all rows from maps > maps_to_keep
        // provider
        //     .tx_ref()
        //     .unwind_table::<tables::FilterMapBaseRowGroup, _>(maps_to_keep, |global_row_index| {
        //         global_row_index / self.params.map_height()
        //     })?;

        // let new_metadata = FilterMapMetadata {
        //     first_indexed_block: metadata.first_indexed_block,
        //     last_indexed_block,
        //     first_map_index: metadata.first_map_index,
        //     last_map_index: unwind_map_index.saturating_sub(1),
        //     next_log_value_index: unwind_map_index << self.params.log_values_per_map,
        // };

        // provider.store_metadata(new_metadata).map_err(|e| StageError::Fatal(Box::new(e)))?;

        Ok(UnwindOutput { checkpoint: StageCheckpoint::new(unwind_to) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        stage_test_suite_ext, ExecuteStageTestRunner, StageTestRunner, StorageKind,
        TestRunnerError, TestStageDB, UnwindStageTestRunner,
    };
    use assert_matches::assert_matches;
    use reth_primitives_traits::SealedBlock;
    use reth_provider::StaticFileWriter;
    use reth_testing_utils::generators::{
        self, random_block_range, random_receipt, BlockRangeParams,
    };

    // Implement stage test suite.
    stage_test_suite_ext!(IndexLogsTestRunner, index_logs);

    #[tokio::test]
    async fn execute_logs_stage() {
        // Initialize logging for the test
        reth_tracing::init_test_tracing();
        let (previous_stage, stage_progress) = (500, 100);
        let mut rng = generators::rng();

        // Set up the runner
        let runner = IndexLogsTestRunner::default();
        let input = ExecInput {
            target: Some(previous_stage),
            checkpoint: Some(StageCheckpoint::new(stage_progress)),
        };

        let blocks = random_block_range(
            &mut rng,
            1..=previous_stage,
            BlockRangeParams { tx_count: 2..3, ..Default::default() },
        );
        runner.db.insert_blocks(blocks.iter(), StorageKind::Static).expect("insert blocks");

        let mut receipts = Vec::new();
        for block in &blocks {
            receipts.reserve_exact(block.transaction_count());
            for transaction in &block.body().transactions {
                receipts.push((
                    receipts.len() as u64,
                    random_receipt(&mut rng, transaction, Some(1), Some(4)),
                ));
            }
        }
        runner.db.insert_receipts(receipts).expect("insert receipts");

        let rx = runner.execute(input);

        // Assert the successful result
        let result = rx.await.unwrap();
        let metadata = &runner.db.table::<tables::LogFilterMetadata>().unwrap()[0].1;

        info!(
            target: "sync::stages::index_logs",
            "Metadata: {:?}",
            metadata
        );

        assert_eq!(metadata.last_indexed_block, previous_stage);

        assert_matches!(result, Ok(ExecOutput {
            checkpoint: StageCheckpoint {
                block_number,
                stage_checkpoint: None
            }, done: true }) if block_number == previous_stage
        );

        // Validate the stage execution
        assert!(runner.validate_execution(input, result.ok()).is_ok(), "execution validation");
    }

    struct IndexLogsTestRunner {
        db: TestStageDB,
        commit_threshold: u64,
    }

    impl UnwindStageTestRunner for IndexLogsTestRunner {
        fn validate_unwind(&self, input: UnwindInput) -> Result<(), TestRunnerError> {
            Ok(())
        }
    }

    impl Default for IndexLogsTestRunner {
        fn default() -> Self {
            Self { db: TestStageDB::default(), commit_threshold: 10 }
        }
    }

    impl StageTestRunner for IndexLogsTestRunner {
        type S = IndexLogsStage;

        fn db(&self) -> &TestStageDB {
            &self.db
        }

        fn stage(&self) -> Self::S {
            IndexLogsStage::new(IndexLogsConfig::default(), Arc::new(FilterMapParams::range_test()))
        }
    }

    impl ExecuteStageTestRunner for IndexLogsTestRunner {
        type Seed = Vec<SealedBlock<reth_ethereum_primitives::Block>>;

        fn seed_execution(&mut self, input: ExecInput) -> Result<Self::Seed, TestRunnerError> {
            let stage_progress = input.checkpoint().block_number;
            let start = stage_progress + 1;
            let end = input.target();

            let mut rng = generators::rng();

            // Generate blocks with txs that will generate logs
            let blocks = random_block_range(
                &mut rng,
                0..=end,
                BlockRangeParams { tx_count: 5..20, ..Default::default() },
            );

            self.db.insert_blocks(blocks.iter(), StorageKind::Static).expect("insert blocks");
            Ok(blocks)
        }

        fn validate_execution(
            &self,
            input: ExecInput,
            output: Option<ExecOutput>,
        ) -> Result<(), TestRunnerError> {
            Ok(())
        }
    }
}
