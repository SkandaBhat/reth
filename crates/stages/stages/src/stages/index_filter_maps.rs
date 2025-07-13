//! Stage for indexing filter maps (EIP-7745).
//!
//! This stage builds filter maps from executed blocks to enable efficient log querying.

use reth_provider::DBProvider;
use reth_stages_api::{
    ExecInput, ExecOutput, Stage, StageCheckpoint, StageError, StageId, UnwindInput, UnwindOutput,
};
use std::fmt::Debug;

/// The filter maps indexing stage.
///
/// This stage processes blocks and their logs to build filter maps according to EIP-7745.
#[derive(Debug, Clone)]
pub struct IndexFilterMapsStage {
    /// The filter map parameters.
    // TODO: Add FilterMapParams when implementing
    /// The current log value index.
    current_lv_index: u64,
}

impl IndexFilterMapsStage {
    /// Creates a new filter maps indexing stage.
    pub const fn new() -> Self {
        Self { current_lv_index: 0 }
    }
}

impl Default for IndexFilterMapsStage {
    fn default() -> Self {
        Self::new()
    }
}

impl<Provider> Stage<Provider> for IndexFilterMapsStage
where
    Provider: DBProvider,
{
    /// Return the id of the stage.
    fn id(&self) -> StageId {
        StageId::Other("IndexFilterMaps")
    }

    /// Execute the stage.
    fn execute(
        &mut self,
        _provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        let target_block = input.target();
        let checkpoint = input.checkpoint();
        let _start_block = checkpoint.block_number + 1;

        // TODO: Implement in stage PR
        // 1. Initialize FilterMapsProcessor with current lv_index
        // 2. For each block in range: a. Load block receipts b. Process block with
        //    processor.process_block() c. Store completed filter maps d. Update block lv pointers
        // 3. Update filter maps metadata
        // 4. Update checkpoint

        // For now, just return done
        Ok(ExecOutput::done(StageCheckpoint::new(target_block)))
    }

    /// Unwind the stage.
    fn unwind(
        &mut self,
        _provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let unwind_to = input.unwind_to;

        // TODO: Implement in stage PR
        // 1. Remove filter map data for blocks > unwind_to
        // 2. Update filter maps metadata
        // 3. Restore lv_index to the correct position

        Ok(UnwindOutput { checkpoint: StageCheckpoint::new(unwind_to) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stage_id() {
        let stage = IndexFilterMapsStage::new();
        assert_eq!(stage.id(), StageId::Other("IndexFilterMaps"));
    }
}
