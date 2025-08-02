use std::fs::Metadata;
use std::{iter::StepBy, ops::RangeInclusive, sync::Arc};

use alloy_consensus::BlockHeader;
use alloy_primitives::{Address, Log, B256};
use alloy_rpc_types_eth::Filter;
use alloy_rpc_types_eth::{BlockHashOrNumber, BlockNumHash};
use reth_log_index::{query_logs, FilterError, FilterMapsReader, FilterResult};
use reth_provider::{BlockHashReader, HeaderProvider, ProviderError, ReceiptProvider};

use crate::{query::get_log_at_index, storage::InMemoryFilterMapsProvider};

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
    filter: Filter,
    from_block: u64,
    to_block: u64,
) -> FilterResult<Vec<Log>> {
    let address: Address = *filter.address.to_value_or_array().unwrap().as_value().unwrap();
    let topics: Vec<B256> = filter
        .topics
        .iter()
        .map(|topic| *topic.to_value_or_array().unwrap().as_value().unwrap())
        .collect();

    let metadata = provider.get_metadata()?;

    // Calculate indexed range (intersection with indexed data)
    let indexed_range = match metadata {
        Some(meta)
            if from_block <= meta.last_indexed_block && to_block >= meta.first_indexed_block =>
        {
            Some(from_block.max(meta.first_indexed_block)..=to_block.min(meta.last_indexed_block))
        }
        _ => None,
    };

    // Calculate bloom ranges (intersection with indexed data)
    let bloom_ranges = match &indexed_range {
        Some(indexed) => {
            let mut ranges = Vec::new();

            if from_block < *indexed.start() {
                ranges.push(from_block..=(*indexed.start() - 1));
            }

            if to_block > *indexed.end() {
                ranges.push((*indexed.end() + 1)..=to_block);
            }

            ranges
        }
        None => vec![from_block..=to_block],
    };

    println!("indexed_range: {:?}", indexed_range);
    println!("bloom_ranges: {:?}", bloom_ranges);

    // Fetch from index (if available)
    let index_future = if let Some(range) = indexed_range {
        Some(tokio::spawn(get_logs_from_indexed_range(provider.clone(), range, address, topics)))
    } else {
        None
    };

    // Fetch from bloom filters
    let bloom_futures: Vec<_> = bloom_ranges
        .into_iter()
        .map(|range| {
            let provider_clone = provider.clone();
            let filter_clone = filter.clone();
            tokio::spawn(async move {
                get_logs_in_block_range_bloom(
                    provider_clone,
                    &filter_clone,
                    *range.start(),
                    *range.end(),
                )
                .await
            })
        })
        .collect();

    // If we have an index future, add it to our list of futures to await
    let mut all_futures = Vec::new();

    if let Some(index_future) = index_future {
        all_futures.push(index_future);
    }

    // Add all bloom futures
    all_futures.extend(bloom_futures);

    // Wait for all futures to complete
    let results = futures::future::join_all(all_futures).await;

    // Collect and flatten all logs
    let mut all_logs = Vec::new();
    for result in results {
        let logs = result.map_err(|e| FilterError::Database(format!("Task error: {}", e)))?;
        all_logs.extend(logs);
    }

    // flatten the logs
    let all_logs = all_logs.into_iter().flatten().collect();
    // TODO: should we sort the logs?

    Ok(all_logs)
}

async fn get_logs_from_indexed_range(
    provider: Arc<InMemoryFilterMapsProvider>,
    index_range: RangeInclusive<u64>,
    address: Address,
    topics: Vec<B256>,
) -> FilterResult<Vec<Log>> {
    let mut logs = Vec::new();

    let log_indices = query_logs(provider.clone(), index_range, address, topics)?;

    println!("log_indices: {:?}", log_indices);

    for log_index in log_indices {
        println!("getting log at index: {:?}", log_index);
        let log = get_log_at_index(provider.clone(), log_index);
        if let Ok(Some(log)) = log {
            if log.address == address {
                logs.push(log);
            } else {
                println!("log address mismatch: {:?}, {:?}", log.address, address);
            }
        }
    }

    println!("returning logs from indexed range: {:?}", logs.len());

    Ok(logs)
}

async fn get_logs_in_block_range_bloom(
    provider: Arc<InMemoryFilterMapsProvider>,
    filter: &Filter,
    from_block: u64,
    to_block: u64,
) -> FilterResult<Vec<Log>> {
    println!("getting logs in bloom filter from {} to {}", from_block, to_block);
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

    println!("returning logs from bloom filter: {:?}", all_logs.len());

    Ok(all_logs)
}
