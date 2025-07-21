//! Integration test for filter maps implementation

mod filtermaps;
mod indexer;
mod storage;

use alloy_primitives::BlockNumber;

use reth_filter_maps::query_logs;

use crate::filtermaps::{create_test_provider_with_random_blocks_and_receipts, get_random_log};
use crate::indexer::index;
use crate::storage::InMemoryFilterMapsProvider;
use reth_filter_maps::{FilterMapParams, FilterMapsProcessor, FilterResult};
use reth_provider::test_utils::MockEthProvider;
use reth_provider::{BlockReader, ReceiptProvider};
use std::ops::RangeInclusive;
use std::sync::Arc;

use alloy_primitives::B256;

const START_BLOCK: BlockNumber = 0;
const BLOCKS_COUNT: usize = 100;

#[tokio::main]
async fn main() {}
