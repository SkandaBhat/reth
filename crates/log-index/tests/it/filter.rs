use std::{iter::StepBy, ops::RangeInclusive, sync::Arc};

use crate::query::fetch_logs_from_index_result;
use alloy_consensus::BlockHeader;
use alloy_primitives::Log;
use alloy_rpc_types_eth::{BlockHashOrNumber, Filter};
use reth_log_index::{
    query::query_logs_in_block_range, FilterMapParams, FilterMapsReader, FilterResult,
};
use reth_provider::{HeaderProvider, ReceiptProvider};
use tracing::{info, trace};

use crate::storage::InMemoryFilterMapsProvider;

const MAX_HEADERS_RANGE: u64 = 1_000; // with ~530bytes per header this is ~500kb

/// An iterator that yields _inclusive_ block ranges of a given step size
#[derive(Debug)]
struct BlockRangeInclusiveIter {
    iter: StepBy<RangeInclusive<u64>>,
    step: u64,
    end: u64,
}

impl BlockRangeInclusiveIter {
    fn new(range: RangeInclusive<u64>, step: u64) -> Self {
        Self { end: *range.end(), iter: range.step_by(step as usize + 1), step }
    }
}

impl Iterator for BlockRangeInclusiveIter {
    type Item = (u64, u64);

    fn next(&mut self) -> Option<Self::Item> {
        let start = self.iter.next()?;
        let end = (start + self.step).min(self.end);
        if start > end {
            return None;
        }
        Some((start, end))
    }
}

pub async fn get_logs_in_block_range(
    provider: Arc<InMemoryFilterMapsProvider>,
    filter: &Filter,
    from_block: u64,
    to_block: u64,
    force_bloom: bool,
) -> FilterResult<Vec<Log>> {
    let params = FilterMapParams::default();
    if force_bloom {
        return get_logs_in_block_range_bloom(provider, filter, from_block, to_block).await;
    }

    let metadata = provider.get_metadata()?.unwrap();

    let indexed_range =
        if from_block <= metadata.last_indexed_block && to_block >= metadata.first_indexed_block {
            Some(
                from_block.max(metadata.first_indexed_block)..=
                    to_block.min(metadata.last_indexed_block),
            )
        } else {
            None
        };

    let mut all_logs: Vec<Log> = Vec::new();

    match indexed_range {
        Some(indexed) => {
            let indexed_start = *indexed.start();
            let indexed_end = *indexed.end();
            // Process in chronological order:

            // 1. First bloom range (before indexed data)
            if from_block < indexed_start {
                let logs = get_logs_in_block_range_bloom(
                    provider.clone(),
                    filter,
                    from_block,
                    indexed_start.saturating_sub(1),
                )
                .await?;
                all_logs.extend(logs);
            }

            // 2. Indexed range (middle)
            let results = query_logs_in_block_range(
                provider.clone(),
                &params,
                &filter,
                indexed_start,
                indexed_end - 1,
            )?;

            for result in results {
                info!("Fetched log from index: {:?}", result);
                if let Some(log) = fetch_logs_from_index_result(provider.clone(), result)? {
                    all_logs.push(log);
                }
            }

            // 3. Second bloom range (after indexed data)
            if to_block > indexed_end {
                let logs =
                    get_logs_in_block_range_bloom(provider.clone(), filter, indexed_end, to_block)
                        .await?;
                all_logs.extend(logs);
            }
        }
        None => {
            // No indexed data, use bloom for entire range
            let logs =
                get_logs_in_block_range_bloom(provider.clone(), filter, from_block, to_block)
                    .await?;
            all_logs.extend(logs);
        }
    }

    Ok(all_logs)
}

async fn get_logs_in_block_range_bloom(
    provider: Arc<InMemoryFilterMapsProvider>,
    filter: &Filter,
    from_block: u64,
    to_block: u64,
) -> FilterResult<Vec<Log>> {
    let mut all_logs = Vec::new();

    // loop over the range of new blocks and check logs if the filter matches the log's bloom
    // filter
    for (from, to) in BlockRangeInclusiveIter::new(from_block..=to_block, MAX_HEADERS_RANGE) {
        let headers = provider.provider.headers_range(from..=to)?;
        for (idx, header) in headers
            .iter()
            .enumerate()
            .filter(|(_, header)| filter.matches_bloom(header.logs_bloom()))
        {
            if let Some(receipts) =
                provider.provider.receipts_by_block(BlockHashOrNumber::Number(header.number()))?
            {
                for receipt in receipts {
                    for log in receipt.logs {
                        if filter.matches(&log) {
                            all_logs.push(log);
                        }
                    }
                }
            }
        }
    }

    Ok(all_logs)
}
