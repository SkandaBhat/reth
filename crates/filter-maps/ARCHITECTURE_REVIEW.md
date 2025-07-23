# Filter Maps Architecture Review

## Executive Summary

This document provides a comprehensive architecture review of the filter-maps crate implementation for EIP-7745 in reth. The review identifies key architectural issues and proposes improvements for better integration with reth's existing patterns, particularly the stages framework.

## Current Architecture Analysis

### 1. API Design Issues

Looking at the integration test in `indexer.rs`, there are several manual steps required:

```rust
// Manual finalization needed
if let Some(map) = indexer.finalize_current_map()? {
    println!("Finalized last map with index: {}", map.map_index);
}

// Manual range metadata update
let range_metadata = reth_filter_maps::storage::FilterMapsRange {
    head_indexed: true,
    blocks_first: first_block,
    blocks_after_last: last_block + 1,
    head_delimiter: current_lv_index,
    // ... manual calculation of fields
};
storage.as_ref().update_filter_maps_range(range_metadata)?;
```

**Issues:**
- Too much manual bookkeeping
- Error-prone (easy to forget steps)
- The processor should handle this internally

### 2. Storage Trait Design

The current storage traits are too granular:

```rust
pub trait FilterMapsReader {
    fn get_filter_map_rows(&self, map_index: u32, row_indices: &[u32]) -> FilterResult<Vec<FilterMapRow>>;
    fn get_block_lv_pointer(&self, block: BlockNumber) -> FilterResult<Option<u64>>;
    fn get_filter_map_last_block(&self, map_index: u32) -> FilterResult<Option<FilterMapLastBlock>>;
    fn get_filter_maps_range(&self) -> FilterResult<Option<FilterMapsRange>>;
}
```

**Issues:**
- Multiple round trips for related data
- No batch operations
- Complex implementation in `storage.rs` shows the pain points

### 3. Provider Interface Confusion

The `FilterMapProvider` trait mixes concerns:

```rust
pub trait FilterMapProvider: Send + Sync {
    fn params(&self) -> &FilterMapParams;
    fn block_to_log_index(&self, block_number: BlockNumber) -> ProviderResult<u64>;
    fn get_filter_rows(&self, map_indices: &[u32], row_index: u32, layer: u32) -> ProviderResult<Vec<FilterRow>>;
    fn get_log(&self, log_index: u64) -> ProviderResult<Option<Log>>;
}
```

**Issues:**
- Mixes filter map access with log retrieval
- The `get_log` method doesn't belong in a filter map provider
- Complex implementation of `block_to_log_index` in test shows design issues

### 4. Type Safety Issues

```rust
// From lib.rs
pub type FilterMap = Vec<FilterRow>;

pub trait FilterMapExt {
    fn to_storage_rows(&self) -> Vec<(u32, FilterMapRow)>;
}
```

**Issues:**
- Type alias doesn't provide type safety
- Extension trait pattern is a code smell
- Should be a proper newtype wrapper

### 5. Database Integration Issues

Looking at the database tables:

```rust
// From tables/mod.rs
table FilterMapRows {
    type Key = u64;  // This is a compound key (map_index, row_index) packed into u64
    type Value = StoredFilterMapRow;
}
```

**Issues:**
- Key encoding is implicit and error-prone
- No type safety for the compound key
- Should use a proper compound key type

### 6. Stage Implementation Problems

The existing `IndexFilterMapsStage` is just a skeleton:

```rust
pub struct IndexFilterMapsStage {
    params: Arc<FilterMapParams>,
    current_lv_index: u64,  // This shouldn't be in the stage!
}
```

**Issues:**
- Stage holds state (`current_lv_index`) that should be in the database
- No proper integration with the storage layer
- Missing unwind logic
- Doesn't follow reth's provider pattern

## Proposed Architecture

### 1. Align with Reth's Provider Pattern

Add proper provider methods:

```rust
// In reth_provider traits
pub trait FilterMapsProvider: Send + Sync {
    /// Write a batch of filter map rows
    fn write_filter_map_batch(
        &self,
        rows: Vec<(MapRowKey, StoredFilterMapRow)>,
    ) -> ProviderResult<()>;
    
    /// Read filter map rows for given indices
    fn filter_map_rows(
        &self,
        map_index: u32,
        row_indices: &[u32],
    ) -> ProviderResult<Vec<StoredFilterMapRow>>;
    
    /// Get or create the current filter maps state
    fn filter_maps_state(&self) -> ProviderResult<FilterMapsState>;
    
    /// Update filter maps state
    fn update_filter_maps_state(&self, state: FilterMapsState) -> ProviderResult<()>;
}

// State that persists across stage executions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterMapsState {
    pub current_lv_index: u64,
    pub current_map_index: u32,
    pub range: FilterMapsRange,
}
```

### 2. Fix Database Schema

Create proper compound key type:

```rust
// In models/mod.rs
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapRowKey {
    pub map_index: u32,
    pub row_index: u32,
}

impl Encode for MapRowKey {
    type Encoded = [u8; 8];
    
    fn encode(self) -> Self::Encoded {
        let mut buf = [0u8; 8];
        buf[0..4].copy_from_slice(&self.map_index.to_be_bytes());
        buf[4..8].copy_from_slice(&self.row_index.to_be_bytes());
        buf
    }
}

impl Decode for MapRowKey {
    fn decode(value: &[u8]) -> Result<Self, DatabaseError> {
        if value.len() != 8 {
            return Err(DatabaseError::Decode);
        }
        Ok(Self {
            map_index: u32::from_be_bytes(value[0..4].try_into().unwrap()),
            row_index: u32::from_be_bytes(value[4..8].try_into().unwrap()),
        })
    }
}

// Update table definition
table FilterMapRows {
    type Key = MapRowKey;
    type Value = StoredFilterMapRow;
}
```

### 3. Refactor FilterMapsProcessor for Stages

The processor needs to be stateless and work with checkpoints:

```rust
// In filter-maps crate
pub struct FilterMapsProcessor {
    params: FilterMapParams,
    // Remove all state fields - these go in the database
}

impl FilterMapsProcessor {
    /// Process a range of blocks, returning the new state
    pub fn process_blocks<P: FilterMapsProvider>(
        &self,
        provider: &P,
        blocks: Vec<(BlockNumber, B256, Vec<Receipt>)>,
        mut state: FilterMapsState,
    ) -> FilterResult<(FilterMapsState, ProcessorOutput)> {
        let mut builder = FilterMapBuilder::new(self.params.clone(), state.current_map_index);
        let mut iterator = LogValueIterator::new(blocks[0].0, state.current_lv_index);
        let mut completed_maps = Vec::new();
        
        for (block_number, block_hash, receipts) in blocks {
            // Process block...
        }
        
        // Return new state and output
        Ok((
            FilterMapsState {
                current_lv_index: iterator.lv_index,
                current_map_index: builder.map_index(),
                range: calculate_range(&state.range, &completed_maps),
            },
            ProcessorOutput { completed_maps, block_pointers },
        ))
    }
}
```

### 4. Implement Proper Stage

```rust
// In stages/stages/src/stages/index_filter_maps.rs
pub struct IndexFilterMapsStage {
    params: Arc<FilterMapParams>,
    batch_size: u64,
}

impl<Provider> Stage<Provider> for IndexFilterMapsStage
where
    Provider: DBProvider + FilterMapsProvider + BlockReader + ReceiptProvider,
{
    fn execute(
        &mut self,
        provider: &Provider,
        input: ExecInput,
    ) -> Result<ExecOutput, StageError> {
        let mut processor = FilterMapsProcessor::new(self.params.clone());
        let mut state = provider.filter_maps_state()?;
        
        let (from_block, to_block) = input.next_block_range();
        
        // Process in batches
        for chunk in (from_block..=to_block).step_by(self.batch_size as usize) {
            let chunk_end = (chunk + self.batch_size - 1).min(to_block);
            
            // Load blocks and receipts
            let blocks = load_blocks_with_receipts(provider, chunk..=chunk_end)?;
            
            // Process the batch
            let (new_state, output) = processor.process_blocks(provider, blocks, state)?;
            
            // Write results in a transaction
            provider.with_tx_mut(|tx| {
                // Write filter map rows
                for (map, rows) in output.completed_maps {
                    tx.write_filter_map_batch(rows)?;
                }
                
                // Write block pointers
                for (block, pointer) in output.block_pointers {
                    tx.put::<tables::BlockLvPointers>(block, pointer)?;
                }
                
                // Update state
                tx.update_filter_maps_state(new_state)?;
                Ok(())
            })?;
            
            state = new_state;
        }
        
        Ok(ExecOutput::done(StageCheckpoint::new(to_block)))
    }
    
    fn unwind(
        &mut self,
        provider: &Provider,
        input: UnwindInput,
    ) -> Result<UnwindOutput, StageError> {
        let unwind_to = input.unwind_to;
        
        // Get current state
        let mut state = provider.filter_maps_state()?;
        
        // Find the log value index at unwind_to
        let unwind_lv_index = provider
            .get::<tables::BlockLvPointers>(unwind_to)?
            .ok_or(StageError::MissingData)?;
        
        // Calculate which maps to remove
        let unwind_map_index = (unwind_lv_index >> self.params.log_values_per_map) as u32;
        
        provider.with_tx_mut(|tx| {
            // Remove maps after unwind point
            for map_idx in (unwind_map_index + 1)..state.current_map_index {
                // Remove all rows for this map
                tx.remove_filter_map(map_idx)?;
            }
            
            // Remove block pointers after unwind_to
            let mut cursor = tx.cursor_write::<tables::BlockLvPointers>()?;
            cursor.seek(unwind_to + 1)?;
            while cursor.next()?.is_some() {
                cursor.delete_current()?;
            }
            
            // Update state
            state.current_lv_index = unwind_lv_index;
            state.current_map_index = unwind_map_index;
            state.range = recalculate_range_after_unwind(&state.range, unwind_to);
            
            tx.update_filter_maps_state(state)?;
            Ok(())
        })?;
        
        Ok(UnwindOutput { checkpoint: StageCheckpoint::new(unwind_to) })
    }
}
```

### 5. Simplified Storage Trait

```rust
// Single unified storage trait for filter-maps crate
pub trait FilterMapsStorage: Send + Sync {
    // Batch operations for efficiency
    fn write_batch(&self, batch: FilterMapsBatch) -> FilterResult<()>;
    
    // Single method to read all data for a range of maps
    fn read_maps(&self, map_range: Range<u32>) -> FilterResult<FilterMapsData>;
    
    // Atomic range update
    fn update_range(&self, range: FilterMapsRange) -> FilterResult<()>;
}

pub struct FilterMapsBatch {
    pub rows: Vec<(MapRowKey, FilterMapRow)>,
    pub block_pointers: Vec<(BlockNumber, u64)>,
    pub last_blocks: Vec<(u32, FilterMapLastBlock)>,
}
```

### 6. Separate Log Provider

```rust
// Separate concerns
pub trait LogProvider: Send + Sync {
    fn get_log(&self, log_index: u64) -> ProviderResult<Option<Log>>;
    fn get_logs_range(&self, start: u64, end: u64) -> ProviderResult<Vec<Log>>;
}

// Simplified FilterMapProvider for queries
pub trait FilterMapQueryProvider: Send + Sync {
    fn params(&self) -> &FilterMapParams;
    fn get_filter_rows(&self, requests: &[FilterRowRequest]) -> ProviderResult<Vec<FilterRow>>;
}
```

### 7. Query Interface for Staged Data

```rust
// In filter-maps crate
pub struct FilterMapsQuery<P> {
    provider: P,
}

impl<P: FilterMapsProvider + BlockReader> FilterMapsQuery<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
    
    pub fn query_logs(
        &self,
        filter: &LogFilter,
    ) -> ProviderResult<Vec<Log>> {
        let state = self.provider.filter_maps_state()?;
        
        // Check if blocks are indexed
        if !state.range.contains_blocks(filter.from_block, filter.to_block) {
            return Err(ProviderError::FilterMapsNotIndexed);
        }
        
        // Build matcher and execute query
        let matcher = build_matcher(&self.provider, filter)?;
        let results = execute_query(&self.provider, matcher, filter)?;
        
        Ok(results)
    }
}
```

### 8. Integration with Stage Sets

```rust
// In stages/stages/src/sets.rs
impl<Provider> DefaultStages<Provider> 
where 
    Provider: StageSetProvider,
{
    pub fn with_filter_maps(mut self, params: FilterMapParams) -> Self {
        // Insert after execution but before finish
        let filter_maps_stage = IndexFilterMapsStage::new(
            Arc::new(params),
            self.stages_config.filter_maps.batch_size,
        );
        
        self.stages.insert_after(
            StageId::Execution,
            Box::new(filter_maps_stage),
        );
        
        self
    }
}
```

## Key Architecture Improvements

1. **Stateless Processor**: The processor no longer holds state between invocations
2. **Proper Provider Integration**: Aligns with reth's provider pattern
3. **Transactional Consistency**: All writes happen in database transactions
4. **Proper Unwind Support**: Can correctly revert state during reorgs
5. **Checkpoint Integration**: Works with stage checkpoints for resumability
6. **Batch Processing**: Efficient processing of blocks in configurable batches
7. **Type Safety**: Proper compound keys instead of packed u64s
8. **Separation of Concerns**: Filter map access separated from log retrieval
9. **Better Error Handling**: Proper error types that integrate with reth
10. **Simplified API**: Reduced manual bookkeeping for users

## Migration Path

1. **Phase 1: Database Schema**
   - Add proper compound key types
   - Update table definitions
   - Create migration for existing data

2. **Phase 2: Provider Integration**
   - Add FilterMapsProvider trait to reth_provider
   - Implement trait for DatabaseProvider
   - Add necessary database methods

3. **Phase 3: Refactor Processor**
   - Make processor stateless
   - Move state to database
   - Update API to work with provider

4. **Phase 4: Stage Implementation**
   - Implement proper execute and unwind methods
   - Add integration tests using stage testing framework
   - Benchmark performance

5. **Phase 5: Query Interface**
   - Implement efficient query methods
   - Add caching layer if needed
   - Performance optimization

6. **Phase 6: Production Rollout**
   - Enable feature flag for filter maps
   - Gradual rollout with monitoring
   - Performance tuning based on real data

## Performance Considerations

1. **Batch Size Tuning**: The stage batch size should be tuned based on:
   - Memory constraints
   - Database write performance
   - Network I/O patterns

2. **Caching Strategy**: Consider adding:
   - LRU cache for frequently accessed filter rows
   - Block pointer cache for recent blocks
   - Pre-computed indices for common queries

3. **Parallel Processing**: Future optimization could include:
   - Parallel filter map construction
   - Concurrent query execution
   - Background index optimization

## Security Considerations

1. **Resource Limits**: Implement limits on:
   - Maximum query range
   - Number of concurrent queries
   - Memory usage per query

2. **Access Control**: Consider:
   - Rate limiting for queries
   - Authentication for write operations
   - Audit logging for sensitive operations

## Conclusion

The proposed architecture addresses the key issues identified in the current implementation while properly integrating with reth's existing patterns. The migration path allows for incremental adoption while maintaining backward compatibility. The design prioritizes correctness, performance, and maintainability, setting a solid foundation for the filter maps feature in reth.