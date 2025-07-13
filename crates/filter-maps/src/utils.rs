use alloy_primitives::{Address, B256};
use sha2::{Digest, Sha256};

/// Compute the log value hash of a log emitting address.
pub fn address_value(address: &Address) -> B256 {
    let mut hasher = Sha256::new();
    hasher.update(address.as_slice());
    B256::from_slice(&hasher.finalize())
}

/// Compute the log value hash of a log topic.
pub fn topic_value(topic: &B256) -> B256 {
    let mut hasher = Sha256::new();
    hasher.update(topic.as_slice());
    B256::from_slice(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, b256};

    #[test]
    fn test_address_value() {
        let addr = address!("0000000000000000000000000000000000000001");
        let value = address_value(&addr);

        // Should be deterministic
        let value2 = address_value(&addr);
        assert_eq!(value, value2);

        // Should be different from the input
        assert_ne!(value.as_slice(), addr.as_slice());
    }

    #[test]
    fn test_topic_value() {
        let topic = b256!("0000000000000000000000000000000000000000000000000000000000000001");
        let value = topic_value(&topic);

        // Should be deterministic
        let value2 = topic_value(&topic);
        assert_eq!(value, value2);

        // Should be different from the input
        assert_ne!(value, topic);
    }
}
