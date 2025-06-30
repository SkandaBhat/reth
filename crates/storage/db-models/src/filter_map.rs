//! Database models for filter map storage based on EIP-7745.

use alloc::vec::Vec;
use alloy_primitives::BlockNumber;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of log values per filter map (EIP-7745)
pub const VALUES_PER_MAP: u64 = 1 << 16; // 65,536 log values

/// Number of rows per filter map (EIP-7745)
pub const MAP_HEIGHT: u64 = 1 << 16; // 65,536 rows

/// Number of columns per filter map (EIP-7745)
pub const MAP_WIDTH: u64 = 1 << 24; // 16,777,216 columns

/// Number of maps per epoch (EIP-7745)
pub const MAPS_PER_EPOCH: u32 = 1 << 10; // 1,024 maps

/// Layer common ratio for mapping layers (EIP-7745)
pub const LAYER_COMMON_RATIO: u32 = 2;

/// Maximum base row length (EIP-7745)
pub const MAX_BASE_ROW_LENGTH: u32 = 256;

/// Compressed filter row data stored in the database.
/// Each row represents a bitmap of log positions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
pub struct CompressedFilterRow {
    /// Compressed row data (roaring bitmap or similar)
    pub data: Vec<u8>,
}

/// Global filter map metadata
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
pub struct FilterMapMetadata {
    /// Highest block number that is fully indexed
    pub indexed_height: BlockNumber,
    /// Total number of log values indexed
    pub total_log_values: u64,
    /// Total number of filter maps (stored as u64 for codec compatibility)
    pub total_maps: u64,
    /// Index version for future upgrades (stored as u64 for codec compatibility)
    pub version: u64,
}

/// Boundary information for a filter map
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
pub struct MapBoundary {
    /// Last block number in this map
    pub last_block: BlockNumber,
    /// Last log value index in this map
    pub last_log_value_index: u64,
}

/// Log value range for a block
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(any(test, feature = "arbitrary"), derive(arbitrary::Arbitrary))]
pub struct LogValueRange {
    /// First log value index in the block
    pub start_index: u64,
    /// Last log value index in the block (inclusive)
    pub end_index: u64,
}

// Manual Compact implementations are required for the impl_compression_for_compact! macro
#[cfg(any(test, feature = "reth-codec"))]
mod compact_impls {
    use super::*;
    use reth_codecs::Compact;

    impl Compact for CompressedFilterRow {
        fn to_compact<B>(&self, buf: &mut B) -> usize
        where
            B: bytes::BufMut + AsMut<[u8]>,
        {
            self.data.to_compact(buf)
        }

        fn from_compact(buf: &[u8], len: usize) -> (Self, &[u8]) {
            let (data, buf) = Vec::<u8>::from_compact(buf, len);
            (Self { data }, buf)
        }
    }

    impl Compact for FilterMapMetadata {
        fn to_compact<B>(&self, buf: &mut B) -> usize
        where
            B: bytes::BufMut + AsMut<[u8]>,
        {
            // Similar to LogValueRange, encode field lengths first
            let mut tmp_height = Vec::new();
            let mut tmp_values = Vec::new();
            let mut tmp_maps = Vec::new();
            let mut tmp_version = Vec::new();

            let len_height = self.indexed_height.to_compact(&mut tmp_height);
            let len_values = self.total_log_values.to_compact(&mut tmp_values);
            let len_maps = self.total_maps.to_compact(&mut tmp_maps);
            let len_version = self.version.to_compact(&mut tmp_version);

            // Write lengths (4 bytes for 4 fields)
            buf.put_u8(len_height as u8);
            buf.put_u8(len_values as u8);
            buf.put_u8(len_maps as u8);
            buf.put_u8(len_version as u8);

            // Write actual data
            buf.put_slice(&tmp_height);
            buf.put_slice(&tmp_values);
            buf.put_slice(&tmp_maps);
            buf.put_slice(&tmp_version);

            4 + len_height + len_values + len_maps + len_version
        }

        fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
            // Read field lengths
            let len_height = buf[0] as usize;
            let len_values = buf[1] as usize;
            let len_maps = buf[2] as usize;
            let len_version = buf[3] as usize;
            let mut buf = &buf[4..];

            // Read fields
            let (indexed_height, remaining) = BlockNumber::from_compact(buf, len_height);
            buf = remaining;

            let (total_log_values, remaining) = u64::from_compact(buf, len_values);
            buf = remaining;

            let (total_maps, remaining) = u64::from_compact(buf, len_maps);
            buf = remaining;

            let (version, remaining) = u64::from_compact(buf, len_version);

            (Self { indexed_height, total_log_values, total_maps, version }, remaining)
        }
    }

    impl Compact for MapBoundary {
        fn to_compact<B>(&self, buf: &mut B) -> usize
        where
            B: bytes::BufMut + AsMut<[u8]>,
        {
            // Encode field lengths first
            let mut tmp_block = Vec::new();
            let mut tmp_index = Vec::new();

            let len_block = self.last_block.to_compact(&mut tmp_block);
            let len_index = self.last_log_value_index.to_compact(&mut tmp_index);

            // Write lengths
            buf.put_u8(len_block as u8);
            buf.put_u8(len_index as u8);

            // Write actual data
            buf.put_slice(&tmp_block);
            buf.put_slice(&tmp_index);

            2 + len_block + len_index
        }

        fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
            // Read field lengths
            let len_block = buf[0] as usize;
            let len_index = buf[1] as usize;
            let mut buf = &buf[2..];

            // Read fields
            let (last_block, remaining) = BlockNumber::from_compact(buf, len_block);
            buf = remaining;

            let (last_log_value_index, remaining) = u64::from_compact(buf, len_index);

            (Self { last_block, last_log_value_index }, remaining)
        }
    }

    impl Compact for LogValueRange {
        fn to_compact<B>(&self, buf: &mut B) -> usize
        where
            B: bytes::BufMut + AsMut<[u8]>,
        {
            // Encode field lengths first (similar to how the derive macro works)
            // We use a simple format: 1 byte for each field length
            let mut tmp_start = Vec::new();
            let mut tmp_end = Vec::new();

            let len_start = self.start_index.to_compact(&mut tmp_start);
            let len_end = self.end_index.to_compact(&mut tmp_end);

            // Write lengths as single bytes (works for values up to 255 bytes)
            buf.put_u8(len_start as u8);
            buf.put_u8(len_end as u8);

            // Write actual data
            buf.put_slice(&tmp_start);
            buf.put_slice(&tmp_end);

            2 + len_start + len_end
        }

        fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
            // Read field lengths
            let len_start = buf[0] as usize;
            let len_end = buf[1] as usize;
            let mut buf = &buf[2..];

            // Read start_index
            let (start_index, remaining) = u64::from_compact(buf, len_start);
            buf = remaining;

            // Read end_index
            let (end_index, remaining) = u64::from_compact(buf, len_end);

            (Self { start_index, end_index }, remaining)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_codecs::Compact;

    #[test]
    fn test_log_value_range_compact() {
        // Test case 1: both non-zero
        let range1 = LogValueRange { start_index: 100, end_index: 200 };
        let mut buf1 = Vec::new();
        range1.to_compact(&mut buf1);
        println!("Range(100,200) encoded to: {:?}", buf1);
        let (decoded1, _) = LogValueRange::from_compact(&buf1, buf1.len());
        assert_eq!(decoded1.start_index, 100);
        assert_eq!(decoded1.end_index, 200);

        // Test case 2: start is zero
        let range2 = LogValueRange { start_index: 0, end_index: 10 };
        let mut buf2 = Vec::new();
        range2.to_compact(&mut buf2);
        println!("Range(0,10) encoded to: {:?}", buf2);
        let (decoded2, _) = LogValueRange::from_compact(&buf2, buf2.len());
        println!(
            "Range(0,10) decoded to: start={}, end={}",
            decoded2.start_index, decoded2.end_index
        );
        assert_eq!(decoded2.start_index, 0);
        assert_eq!(decoded2.end_index, 10);
    }

    #[test]
    fn test_filter_map_metadata_compact() {
        let meta = FilterMapMetadata {
            indexed_height: 1000,
            total_log_values: 65536,
            total_maps: 1,
            version: 1,
        };

        let mut buf = Vec::new();
        meta.to_compact(&mut buf);

        let (decoded, _) = FilterMapMetadata::from_compact(&buf, buf.len());
        assert_eq!(decoded.indexed_height, 1000);
        assert_eq!(decoded.total_log_values, 65536);
        assert_eq!(decoded.total_maps, 1);
        assert_eq!(decoded.version, 1);
    }
}
