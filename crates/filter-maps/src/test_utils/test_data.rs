//! Test data generators for filter maps

use alloy_primitives::{address, Address, B256};

/// Common test addresses
pub const TEST_ADDRESSES: &[Address] = &[
    address!("0x1111111111111111111111111111111111111111"),
    address!("0x2222222222222222222222222222222222222222"),
    address!("0x3333333333333333333333333333333333333333"),
    address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    address!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
    address!("0xcccccccccccccccccccccccccccccccccccccccc"),
];

/// Common test topics
pub const TEST_TOPICS: &[B256] = &[
    B256::repeat_byte(0x11),
    B256::repeat_byte(0x22),
    B256::repeat_byte(0x33),
    B256::repeat_byte(0x44),
    B256::repeat_byte(0xaa),
    B256::repeat_byte(0xbb),
];

/// Generate test blocks with logs (placeholder - to be implemented)
pub fn generate_test_blocks_with_logs(_num_blocks: u64, _logs_per_block: u64) -> Vec<()> {
    vec![]
}

/// Generate receipts with specific log patterns (placeholder - to be implemented)
pub fn generate_receipts_with_logs(_patterns: Vec<Vec<(Address, Vec<B256>)>>) -> Vec<()> {
    vec![]
}

/// Build filter rows from logs (placeholder - to be implemented)
pub fn build_filter_rows_from_logs(_logs: &[()], _params: ()) -> Vec<(u32, Vec<u8>)> {
    vec![]
}