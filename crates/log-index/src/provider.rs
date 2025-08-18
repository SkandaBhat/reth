//! Storage types for filter maps (EIP-7745).
//!
//! This module defines the storage representations of filter maps data
//! that can be stored in the database.

use crate::{
    params::FilterMapParams,
    types::{FilterError, FilterMapMetadata, FilterMapRow, FilterResult, MapRowIndex},
    utils::{intersect_sorted, shift_filter_to_map, union_sorted},
};
use alloy_primitives::{map::HashMap, Address, BlockNumber, B256};
use std::{
    ops::RangeInclusive,
    sync::{Arc, RwLock},
    time::Instant,
    vec::Vec,
};

use tracing::{error, info};

/// A unified cache for filter map data
#[derive(Default)]
pub struct FilterCache {
    // Processed matches: (map_index, value) -> Arc<HashSet<u64>>
    matches: RwLock<HashMap<(u64, B256), Arc<Vec<u64>>>>,
}

impl FilterCache {
    pub fn new() -> Self {
        Self { matches: RwLock::new(HashMap::default()) }
    }

    fn get_or_compute_matches(
        &self,
        params: &FilterMapParams,
        rows: &[FilterMapRow],
        map_index: u64,
        value: &B256,
    ) -> FilterResult<Arc<Vec<u64>>> {
        // Check matches cache first
        if let Some(hit) = self.matches.read().unwrap().get(&(map_index, *value)).cloned() {
            return Ok(hit);
        }

        // Compute matches
        let vec = params.potential_matches(&rows, map_index, value)?;
        let arc = Arc::new(vec);

        // Cache the result
        self.matches.write().unwrap().insert((map_index, *value), arc.clone());
        Ok(arc)
    }
}
/// Provider trait for reading filter map data.
#[auto_impl::auto_impl(&, Arc)]
pub trait FilterMapsReader: Send + Sync {
    /// Get filter map metadata.
    fn get_metadata(&self) -> FilterResult<Option<FilterMapMetadata>>;

    /// Get filter map row for a given map and row index.
    /// Returns None if the row is not found.
    fn get_filter_map_rows(
        &self,
        global_row_indices: Vec<u64>,
    ) -> FilterResult<Option<HashMap<u64, FilterMapRow>>>;

    /// Get the log value index for a given block number.
    fn get_log_value_index_for_block(&self, block: BlockNumber) -> FilterResult<Option<u64>>;

    /// Find which block contains the given log value index, using binary search.
    /// Returns None if the log index is beyond the indexed range.
    ///
    /// The returned tuple contains the block number and the log value index of the first log in the
    /// block.
    fn find_block_for_log_value_index(
        &self,
        metadata: FilterMapMetadata,
        log_value_index: u64,
        range: Option<RangeInclusive<u64>>, // if provided, use this range to limit the search
    ) -> FilterResult<Option<(BlockNumber, u64)>> {
        // Check if log index is beyond the indexed range
        if log_value_index >= metadata.next_log_value_index {
            info!(
                target: "log-index::provider",
                log_value_index,
                "Log value index is beyond the indexed range"
            );
            return Ok(None);
        }

        // Binary search for the block number that contains the log value index
        let mut start = range.clone().map(|r| *r.start()).unwrap_or(metadata.first_indexed_block);
        let mut end = range.map(|r| *r.end()).unwrap_or(metadata.last_indexed_block);
        let mut result = None;

        while start <= end {
            let mid = start + (end - start) / 2;

            // get delimiter for the block
            let block_log_value_index =
                self.get_log_value_index_for_block(BlockNumber::from(mid)).map_err(|e| {
                    error!(
                        target: "log-index::provider",
                        %e,
                        "Failed to get log value index for block"
                    );
                    FilterError::Database("Failed to get log value index for block".to_string())
                });
            if block_log_value_index.is_err() {
                error!(
                    target: "log-index::provider",
                    "Failed to get log value index for block {}",
                    mid
                );
                return Err(FilterError::Database(
                    "Failed to get log value index for block".to_string(),
                ));
            }

            if let Some(block_log_value_index) = block_log_value_index.unwrap() {
                if log_value_index >= block_log_value_index {
                    // this block starts before or at our target
                    result = Some((mid, block_log_value_index));
                    start = mid + 1;
                } else {
                    // this block starts after our target
                    end = mid.saturating_sub(1);
                }
            } else {
                // no delimiter found, this shouldnt happen in valid data
                return Err(FilterError::Database(
                    "No start log value index found for block".to_string(),
                ));
            }
        }

        Ok(result)
    }
    /// Check if a block range is fully indexed.
    fn is_block_range_indexed(&self, range: RangeInclusive<BlockNumber>) -> FilterResult<bool> {
        let metadata = self.get_metadata()?;
        let meta = match metadata {
            Some(meta) => meta,
            None => return Ok(false),
        };

        Ok(range.start() >= &meta.first_indexed_block && range.end() <= &meta.last_indexed_block)
    }

    /// Get the map indices for a given block range.
    fn get_map_indices_for_block_range(
        &self,
        range: RangeInclusive<BlockNumber>,
    ) -> FilterResult<Vec<u64>> {
        let params = FilterMapParams::default(); // TODO: figure out a better way to get the params
        let first_index = self.get_log_value_index_for_block(*range.start())?.unwrap_or_default();
        let last_index = self.get_log_value_index_for_block(*range.end())?.unwrap_or_default();

        let first_map = first_index >> params.log_values_per_map;
        let last_map = last_index >> params.log_values_per_map;
        let map_indices: Vec<u64> = (first_map..=last_map).collect();
        Ok(map_indices)
    }

    /// Query a single map for log indices.
    fn query_maps(
        &self,
        map_indices: Vec<u64>,
        addr_vals: &[B256],
        topic_vals: &[(usize, Vec<B256>)],
    ) -> FilterResult<Vec<u64>> {
        let params = FilterMapParams::default(); // TODO: figure out a better way to get the params
        let shift = params.log_values_per_map;

        let mut global_row_indices = HashMap::new();
        for map_index in map_indices.clone() {
            let mut row_indices = Vec::new();
            for addr_val in addr_vals {
                let row_indices_for_addr = params.global_row_indices(map_index, addr_val);
                row_indices.extend(row_indices_for_addr);
            }
            for (k, vals) in topic_vals {
                for val in vals {
                    let row_indices_for_topic = params.global_row_indices(map_index, val);
                    row_indices.extend(row_indices_for_topic);
                }
            }
            global_row_indices.insert(map_index, row_indices);
        }

        let mut row_indices = global_row_indices.values().flatten().copied().collect::<Vec<u64>>();

        let all_rows = self.fetch_filter_map_rows(row_indices)?.unwrap_or_default();

        let map_indices_rows = map_indices
            .iter()
            .map(|map_index| {
                let indices_for_map =
                    global_row_indices.get(map_index).cloned().unwrap_or_default();
                let rows = indices_for_map
                    .iter()
                    .map(|index| {
                        let row = all_rows.get(index).cloned().unwrap_or(FilterMapRow::default());
                        if row.is_empty() {
                            None
                        } else {
                            Some(row)
                        }
                    })
                    .flatten()
                    .collect::<Vec<_>>();
                (map_index, rows)
            })
            .collect::<HashMap<_, _>>();

        let mut all_candidates: Vec<u64> = Vec::new();

        let cache = FilterCache::new();
        let mut query_map_duration = 0;
        for map_index in map_indices.clone() {
            let rows = map_indices_rows.get(&map_index).cloned().unwrap_or_default();
            let next_rows = map_indices_rows.get(&(map_index + 1)).cloned().unwrap_or_default();
            let start_time = Instant::now();
            let candidates =
                self.query_map(&cache, map_index, &rows, &next_rows, addr_vals, topic_vals)?;
            all_candidates.extend(candidates);
            let end_time = Instant::now();
            let duration = end_time.duration_since(start_time);
            query_map_duration += duration.as_nanos();
        }
        info!(
            target: "log-index::provider",
            duration = query_map_duration / map_indices.len() as u128,
            "query_maps duration for number of map indices: {}",
            map_indices.len()
        );
        Ok(all_candidates)
    }

    fn query_map(
        &self,
        cache: &FilterCache,
        map_index: u64,
        rows: &[FilterMapRow],
        next_rows: &[FilterMapRow],
        addr_vals: &[B256],
        topic_vals: &[(usize, Vec<B256>)],
    ) -> FilterResult<Vec<u64>> {
        let params = FilterMapParams::default(); // TODO: figure out a better way to get the params
        let shift_bits = params.log_values_per_map;

        // Addresses OR
        let mut addr_or: Vec<u64> = Vec::new();
        if !addr_vals.is_empty() {
            for val in addr_vals {
                // matches in THIS map only (addresses have offset 0)
                let address_matches =
                    cache.get_or_compute_matches(&params, rows, map_index, val)?;
                addr_or = if addr_or.is_empty() {
                    (*address_matches).clone() // first vector (already sorted)
                } else {
                    union_sorted(&addr_or, address_matches.as_slice())
                };
            }
            if !addr_or.is_empty() {
                let base = (map_index) << shift_bits;
                let next = base + (1u64 << shift_bits);
                // retain only those in [base, next)
                let mut filtered = Vec::with_capacity(addr_or.len());
                for &x in &addr_or {
                    if x >= base && x < next {
                        filtered.push(x);
                    }
                }
                addr_or = filtered;
                if addr_or.is_empty() {
                    return Ok(Vec::new());
                }
            } else {
                return Ok(Vec::new());
            }
        }

        // Topics: for each position k, OR across allowed values and maps (this + spill),
        // then shift by offset and keep bases in map_index. Defer intersections.
        let mut topic_bases: Vec<Vec<u64>> = Vec::with_capacity(topic_vals.len());

        for (k, allowed) in topic_vals {
            // Union of all values inside the current map
            let mut cur_or: Vec<u64> = Vec::new();
            for val in allowed {
                let topic_matches = cache.get_or_compute_matches(&params, rows, map_index, val)?;
                cur_or = if cur_or.is_empty() {
                    (*topic_matches).clone()
                } else {
                    union_sorted(&cur_or, topic_matches.as_slice())
                };
            }

            // Union of all values in the NEXT map (spill)
            let mut spill_or: Vec<u64> = Vec::new();
            if !allowed.is_empty() {
                for val in allowed {
                    let topic_matches =
                        cache.get_or_compute_matches(&params, next_rows, map_index + 1, val)?;
                    spill_or = if spill_or.is_empty() {
                        (*topic_matches).clone()
                    } else {
                        union_sorted(&spill_or, topic_matches.as_slice())
                    };
                }
            }

            // Merge current+spill (still sorted+dedup)
            let merged =
                if spill_or.is_empty() { cur_or } else { union_sorted(&cur_or, &spill_or) };

            // Shift by offset = 1 + k, keep only bases that belong to `map_index`
            let offset = 1u64 + (*k as u64);
            let bases = shift_filter_to_map(&merged, offset, map_index, shift_bits);

            if bases.is_empty() {
                // Early exit (this topic position constrains to nothing)
                return Ok(Vec::new());
            }
            topic_bases.push(bases);
        }

        // Intersect: address OR (if any) with topic bases; intersect topics shortest-first
        if topic_bases.len() > 1 {
            topic_bases.sort_by_key(|v| v.len());
        }

        let mut candidates = if !addr_or.is_empty() {
            // address AND first topics (if any)
            if topic_bases.is_empty() {
                addr_or
            } else {
                intersect_sorted(&addr_or, &topic_bases[0])
            }
        } else {
            // no addresses: start from first topic set
            if topic_bases.is_empty() {
                // nothing constrained => match-all is handled at a higher layer; here say none
                return Ok(Vec::new());
            }
            topic_bases[0].clone()
        };

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        for tb in topic_bases.iter().skip(1) {
            candidates = intersect_sorted(&candidates, tb);
            if candidates.is_empty() {
                return Ok(Vec::new());
            }
        }
        Ok(candidates)
    }

    /// Performs a log query using `FilterMaps`.
    ///
    /// This is the main entry point for log filtering using the `FilterMaps` data structure.
    ///
    /// # Arguments
    ///
    /// * `range` - The block range to search (inclusive)
    /// * `addresses` - List of addresses to match (empty = match all)
    /// * `topics` - List of topic filters, where each position can have multiple values (OR)
    ///
    /// # Returns
    ///
    /// A list of logs matching the filter criteria. Note that false positives are possible
    /// and should be filtered out by the caller if exact matching is required.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The filter matches all logs (use legacy filtering instead)
    /// - There's an issue accessing the `FilterMaps` data
    fn query_logs(
        &self,
        map_indices: Vec<u64>,
        addresses: Vec<Address>,
        topics: Vec<Vec<B256>>,
    ) -> FilterResult<Vec<u64>> {
        Ok(Vec::new())
    }

    /// Retrieves all rows from a filter map that could potentially contain a given log value.
    ///
    /// # Arguments
    ///
    /// * `map_index` - The index of the filter map to search.
    /// * `log_value` - The log value (as a B256 hash) to search for.
    /// * `params` - The filter map parameters.
    ///
    /// # Returns
    ///
    /// Returns a vector of `FilterMapRow` objects that potentially contain the given log value.
    fn fetch_filter_map_rows(
        &self,
        global_row_indices: Vec<u64>,
    ) -> FilterResult<Option<HashMap<u64, FilterMapRow>>> {
        let start_time = Instant::now();
        let rows = self.get_filter_map_rows(global_row_indices.clone())?;
        let end_time = Instant::now();
        let duration = end_time.duration_since(start_time);
        info!(
            target: "log-index::provider",
            duration = duration.as_millis(),
            "fetch_filter_map_rows duration for number of global row indices: {}",
            global_row_indices.len()
        );
        Ok(rows)
    }
}

/// Provider trait for writing filter map data.
pub trait FilterMapsWriter: Send + Sync {
    /// Store filter map metadata.
    fn store_metadata(&self, metadata: FilterMapMetadata) -> FilterResult<()>;

    /// Store filter map rows.
    fn store_filter_map_row(
        &self,
        map_row_index: MapRowIndex,
        row: FilterMapRow,
    ) -> FilterResult<()>;

    /// Store block to log value pointer mapping.
    fn store_log_value_index_for_block(
        &self,
        block: BlockNumber,
        log_value_index: u64,
    ) -> FilterResult<()>;
}
