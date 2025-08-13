use alloy_consensus::BlockHeader;
use alloy_primitives::{logs_bloom, Address, BlockNumber, Log};
use rand::Rng;
use reth_ethereum_primitives::Receipt;
use reth_provider::test_utils::MockEthProvider;
use reth_testing_utils::generators::{
    random_block_range, random_log, random_receipt, rng_with_seed, BlockRangeParams,
};

pub(crate) async fn create_test_provider_with_random_blocks_and_receipts(
    start_block: BlockNumber,
    blocks_count: usize,
    tx_count: u8,
    log_count: u8,
    max_topics: usize,
) -> MockEthProvider {
    let mut rng = rng_with_seed(b"test_filter_map");

    let provider = MockEthProvider::default();

    let blocks = random_block_range(
        &mut rng,
        start_block..=start_block + blocks_count as u64 - 1,
        BlockRangeParams { tx_count: 0..tx_count, ..Default::default() },
    );

    // Create receipts and calculate bloom filters
    let mut all_receipts: Vec<(BlockNumber, Vec<Receipt>)> = Vec::new();
    let mut updated_blocks = Vec::new();

    for mut block in blocks {
        let mut block_receipts = Vec::new();

        for transaction in block.body().transactions.iter() {
            let mut receipt = random_receipt(&mut rng, transaction, Some(0), None);
            //generate LOG_COUNT logs
            let logs: Vec<Log> = (0..log_count)
                .map(|_| {
                    let address = Address::from(rng.random::<[u8; 20]>());
                    random_log(&mut rng, Some(address), Some(max_topics as u8))
                })
                .collect();
            receipt.logs = logs;
            block_receipts.push(receipt);
        }

        // calculate bloom filter for the block
        let bloom = logs_bloom(block_receipts.iter().flat_map(|r| r.logs.iter()));

        // update the block header with the bloom filter
        block.header_mut().logs_bloom = bloom;

        all_receipts.push((block.number(), block_receipts));
        updated_blocks.push(block);
    }

    provider.extend_blocks(updated_blocks.into_iter().map(|b| (b.hash(), b.into_block())));
    provider.extend_receipts(all_receipts.into_iter());

    provider
}
