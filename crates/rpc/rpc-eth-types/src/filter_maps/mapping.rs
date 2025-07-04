//! Row and column mapping algorithms for FilterMaps based on EIP-7745.
//! see: https://eips.ethereum.org/EIPS/eip-7745

use alloy_primitives::B256;
use fnv::FnvHasher;
use sha2::{Digest, Sha256};
use std::hash::Hasher;

/// A strictly monotonically increasing list of log value indices in the range of a
/// filter map that are potential matches for certain filter criteria.
///
/// Note that nil is used as a wildcard and therefore means that all log value
/// indices in the filter map range are potential matches. If there are no
/// potential matches in the given map's range then an empty slice should be used.
pub type PotentialMatches = Vec<u64>;

/// FilterMaps parameters based on EIP-7745
#[derive(Debug, Clone)]
pub struct FilterMapParams {
    /// Log of map height (rows per map). Default: 16 (65,536 rows)
    pub log_map_height: u32,
    /// Log of map width (columns per row). Default: 24 (16,777,216 columns)
    pub log_map_width: u32,
    /// Log of maps per epoch. Default: 10 (1,024 maps)
    pub log_maps_per_epoch: u32,
    /// Log of values per map. Default: 16 (65,536 values)
    pub log_values_per_map: u32,
    /// baseRowLength / average row length
    pub base_row_length_ratio: u32,
    /// maxRowLength log2 growth per layer
    pub log_layer_diff: u32,

    /// (Not affecting consensus)
    pub base_row_group_length: u32,
}

impl Default for FilterMapParams {
    fn default() -> Self {
        Self {
            log_map_height: 16,
            log_map_width: 24,
            log_maps_per_epoch: 10,
            log_values_per_map: 16,
            base_row_length_ratio: 8,
            log_layer_diff: 4,
            base_row_group_length: 32,
        }
    }
}

impl FilterMapParams {
    /// Get the number of rows per map
    pub fn map_height(&self) -> u32 {
        1 << self.log_map_height
    }

    /// Get the number of columns per row
    pub fn map_width(&self) -> u32 {
        1 << self.log_map_width
    }

    /// Get the number of maps per epoch
    pub fn maps_per_epoch(&self) -> u32 {
        1 << self.log_maps_per_epoch
    }

    /// Get the number of values per map
    pub fn values_per_map(&self) -> u64 {
        1 << self.log_values_per_map
    }

    /// Get the base row length
    pub fn base_row_length(&self) -> u32 {
        ((self.values_per_map() * self.base_row_length_ratio as u64) / self.map_height() as u64)
            as u32
    }

    /// Calculate the row index in which the given log value should be marked
    /// on the given map and mapping layer.
    ///
    /// Note that row assignments are re-shuffled with a different frequency on each
    /// mapping layer, allowing efficient disk access and Merkle proofs for long
    /// sections of short rows on lower order layers while avoiding putting too many
    /// heavy rows next to each other on higher order layers.
    pub fn row_index(&self, map_index: u32, layer_index: u32, log_value: &B256) -> u32 {
        let mut hasher = Sha256::new();

        hasher.update(log_value.as_slice());

        let masked_index = self.masked_map_index(map_index, layer_index);
        let mut index_bytes = [0u8; 8];
        index_bytes[0..4].copy_from_slice(&masked_index.to_le_bytes());
        index_bytes[4..8].copy_from_slice(&layer_index.to_le_bytes());
        hasher.update(&index_bytes);

        let hash = hasher.finalize();

        // Take first 4 bytes and modulo by map height
        let row = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
        row % self.map_height()
    }

    /// Returns the column index where the given log value at the given
    /// position should be marked.
    pub fn column_index(&self, lv_index: u64, log_value: &B256) -> u32 {
        let mut hasher = FnvHasher::default();
        hasher.write(&lv_index.to_le_bytes());
        hasher.write(log_value.as_slice());
        let hash = hasher.finish();

        // Calculate hash bits needed
        let hash_bits = self.log_map_width - self.log_values_per_map;

        // Combine position bits with hash bits
        let position_bits = (lv_index % self.values_per_map()) as u32;
        let hash_component = (hash >> (64 - hash_bits)) as u32 ^ (hash >> (32 - hash_bits)) as u32;

        (position_bits << hash_bits) | hash_component
    }

    /// Returns the index used for row mapping calculation on the given layer.
    ///
    /// On layer zero the mapping changes once per epoch, then the frequency of
    /// re-mapping increases with every new layer until it reaches the frequency
    /// where it is different for every mapIndex.
    pub fn masked_map_index(&self, map_index: u32, layer_index: u32) -> u32 {
        let log_layer_diff = (layer_index * self.log_layer_diff).min(self.log_maps_per_epoch);
        map_index & (u32::MAX << (self.log_maps_per_epoch - log_layer_diff))
    }

    // maxRowLength returns the maximum length filter rows are populated up to
    // when using the given mapping layer. A log value can be marked on the map
    // according to a given mapping layer if the row mapping on that layer points
    // to a row that has not yet reached the maxRowLength belonging to that layer.
    // This means that a row that is considered full on a given layer may still be
    // extended further on a higher order layer.
    // Each value is marked on the lowest order layer possible, assuming that marks
    // are added in ascending log value index order.
    // When searching for a log value one should consider all layers and process
    // corresponding rows up until the first one where the row mapped to the given
    // layer is not full.
    pub fn max_row_length(&self, layer_index: u32) -> u32 {
        let log_layer_diff = (layer_index * self.log_layer_diff).min(self.log_maps_per_epoch);
        self.base_row_length() << log_layer_diff
    }

    // TODO: Implement this
    pub fn potential_matches(&self) -> PotentialMatches {
        todo!()
    }
}

// /// Calculate the global row index for database storage.
// ///
// /// This ensures rows from the same map are stored together.
// pub fn map_row_index(map_index: u32, row_index: u32, params: &FilterMapParams) -> u64 {
//     ((map_index as u64) * (params.map_height() as u64)) + (row_index as u64)
// }

// /// Get the map index from a log value index.
// pub fn map_index_from_lv_index(lv_index: u64, params: &FilterMapParams) -> u32 {
//     (lv_index / params.values_per_map()) as u32
// }

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::b256;

    #[test]
    fn test_masked_map_index() {
        let params = FilterMapParams::default();

        // Layer 0: no masking
        assert_eq!(params.masked_map_index(0b1111, 0), 0b1111);

        // Layer 1: mask lowest bit
        assert_eq!(params.masked_map_index(0b1111, 1), 0b1110);

        // Layer 2: mask lowest 2 bits
        assert_eq!(params.masked_map_index(0b1111, 2), 0b1100);

        // Layer 3: mask lowest 3 bits
        assert_eq!(params.masked_map_index(0b1111, 3), 0b1000);
    }

    #[test]
    fn test_row_index_distribution() {
        let params = FilterMapParams::default();
        let value1 = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let value2 = b256!("0000000000000000000000000000000000000000000000000000000000000002");

        // Same value should always map to same row for same map/layer
        let row1 = params.row_index(0, 0, &value1);
        let row2 = params.row_index(0, 0, &value1);
        assert_eq!(row1, row2);

        // Different values should (likely) map to different rows
        let row3 = params.row_index(0, 0, &value2);
        // Different values should produce different rows in most cases
        // We can't assert they're always different due to possible hash collisions
        // but we can verify they're valid row indices
        assert!(row3 < params.map_height());

        // Different layers should give different row mappings for the same value
        let row_layer0 = params.row_index(0, 0, &value1);
        let row_layer1 = params.row_index(0, 1, &value1);
        let row_layer2 = params.row_index(0, 2, &value1);

        // All should be valid indices
        assert!(row_layer0 < params.map_height());
        assert!(row_layer1 < params.map_height());
        assert!(row_layer2 < params.map_height());

        // At least one pair should differ (extremely unlikely all three are the same)
        assert!(
            row_layer0 != row_layer1 || row_layer1 != row_layer2 || row_layer0 != row_layer2,
            "All three layers produced the same row index, which is extremely unlikely"
        );
    }

    #[test]
    fn test_column_index() {
        let params = FilterMapParams::default();
        let value = b256!("0000000000000000000000000000000000000000000000000000000000000001");

        // Column index should include position information
        let col1 = params.column_index(0, &value);
        let col2 = params.column_index(params.values_per_map(), &value);

        // Both should be in same position (0) but with different hash bits
        assert_eq!(col1 >> 8, 0);
        assert_eq!(col2 >> 8, 0);

        // Different positions
        let col3 = params.column_index(1, &value);
        assert_eq!(col3 >> 8, 1);
    }
}
