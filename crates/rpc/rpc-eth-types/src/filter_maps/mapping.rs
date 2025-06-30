//! Row and column mapping algorithms for FilterMaps based on EIP-7745.

use alloy_primitives::B256;
use std::hash::Hasher;

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
    /// Base row length. Default: 24
    pub base_row_length: u32,
    /// Log of layer difference. Default: 2 (4x growth per layer)
    pub log_layer_diff: u32,
}

impl Default for FilterMapParams {
    fn default() -> Self {
        Self {
            log_map_height: 16,
            log_map_width: 24,
            log_maps_per_epoch: 10,
            log_values_per_map: 16,
            base_row_length: 24,
            log_layer_diff: 2,
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

    /// Get the maximum row length for a given layer
    pub fn max_row_length(&self, layer: u32) -> u32 {
        self.base_row_length << (layer * self.log_layer_diff)
    }
}

/// Calculate the masked map index for row mapping.
///
/// The masking frequency changes based on the layer to provide
/// different row distributions at each layer.
pub fn masked_map_index(map_index: u32, layer_index: u32) -> u32 {
    // Higher layers re-map more frequently
    // Layer 0: no masking
    // Layer 1: mask lowest bit
    // Layer 2: mask lowest 2 bits, etc.
    let mask = !((1u32 << layer_index) - 1);
    map_index & mask
}

/// Calculate the row index for a log value in a specific map and layer.
///
/// Uses SHA256 hash to distribute values across rows uniformly.
pub fn row_index(
    map_index: u32,
    layer_index: u32,
    log_value: &B256,
    params: &FilterMapParams,
) -> u32 {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();

    // Hash the log value
    hasher.update(log_value.as_slice());

    // Include masked map index for re-shuffling at different layers
    let masked_index = masked_map_index(map_index, layer_index);
    let mut index_bytes = [0u8; 8];
    index_bytes[0..4].copy_from_slice(&masked_index.to_le_bytes());
    index_bytes[4..8].copy_from_slice(&layer_index.to_le_bytes());
    hasher.update(&index_bytes);

    let hash = hasher.finalize();

    // Take first 4 bytes and modulo by map height
    let row = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    row % params.map_height()
}

/// Calculate the column index for a log value.
///
/// Combines position within the map with a hash for collision resistance.
pub fn column_index(lv_index: u64, log_value: &B256, params: &FilterMapParams) -> u32 {
    use rustc_hash::FxHasher;

    // Position within the current map
    let position_bits = lv_index % params.values_per_map();

    // Hash the log value index and value together
    let mut hasher = FxHasher::default();
    hasher.write_u64(lv_index);
    hasher.write(log_value.as_slice());
    let hash = hasher.finish();

    // Use 8 bits from the hash for additional entropy
    let hash_bits = (hash & 0xFF) as u32;

    // Combine position (higher bits) with hash (lower bits)
    ((position_bits as u32) << 8) | hash_bits
}

/// Calculate the global row index for database storage.
///
/// This ensures rows from the same map are stored together.
pub fn map_row_index(map_index: u32, row_index: u32, params: &FilterMapParams) -> u64 {
    ((map_index as u64) * (params.map_height() as u64)) + (row_index as u64)
}

/// Get the map index from a log value index.
pub fn map_index_from_lv_index(lv_index: u64, params: &FilterMapParams) -> u32 {
    (lv_index / params.values_per_map()) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::b256;

    #[test]
    fn test_masked_map_index() {
        // Layer 0: no masking
        assert_eq!(masked_map_index(0b1111, 0), 0b1111);

        // Layer 1: mask lowest bit
        assert_eq!(masked_map_index(0b1111, 1), 0b1110);

        // Layer 2: mask lowest 2 bits
        assert_eq!(masked_map_index(0b1111, 2), 0b1100);

        // Layer 3: mask lowest 3 bits
        assert_eq!(masked_map_index(0b1111, 3), 0b1000);
    }

    #[test]
    fn test_row_index_distribution() {
        let params = FilterMapParams::default();
        let value1 = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let value2 = b256!("0000000000000000000000000000000000000000000000000000000000000002");

        // Same value should always map to same row for same map/layer
        let row1 = row_index(0, 0, &value1, &params);
        let row2 = row_index(0, 0, &value1, &params);
        assert_eq!(row1, row2);

        // Different values should (likely) map to different rows
        let row3 = row_index(0, 0, &value2, &params);
        // Different values should produce different rows in most cases
        // We can't assert they're always different due to possible hash collisions
        // but we can verify they're valid row indices
        assert!(row3 < params.map_height());

        // Different layers should give different row mappings for the same value
        let row_layer0 = row_index(0, 0, &value1, &params);
        let row_layer1 = row_index(0, 1, &value1, &params);
        let row_layer2 = row_index(0, 2, &value1, &params);

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
        let col1 = column_index(0, &value, &params);
        let col2 = column_index(params.values_per_map(), &value, &params);

        // Both should be in same position (0) but with different hash bits
        assert_eq!(col1 >> 8, 0);
        assert_eq!(col2 >> 8, 0);

        // Different positions
        let col3 = column_index(1, &value, &params);
        assert_eq!(col3 >> 8, 1);
    }

    #[test]
    fn test_map_row_index() {
        let params = FilterMapParams::default();

        // First row of first map
        assert_eq!(map_row_index(0, 0, &params), 0);

        // First row of second map
        assert_eq!(map_row_index(1, 0, &params), params.map_height() as u64);

        // Last row of first map
        assert_eq!(
            map_row_index(0, params.map_height() - 1, &params),
            (params.map_height() - 1) as u64
        );
    }

    #[test]
    fn test_layer_row_lengths() {
        let params = FilterMapParams::default();

        // Layer 0: base length
        assert_eq!(params.max_row_length(0), 24);

        // Layer 1: 4x base
        assert_eq!(params.max_row_length(1), 96);

        // Layer 2: 16x base
        assert_eq!(params.max_row_length(2), 384);
    }
}
