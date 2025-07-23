/// State of the filter maps processor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FilterMapsState {
    /// Current log value index.
    pub log_value_index: u64,
    /// current map index
    pub map_index: u32,
}

#[cfg(feature = "reth-codecs")]
impl reth_codecs::Compact for FilterMapsState {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: bytes::BufMut + AsMut<[u8]>,
    {
        let mut len = 0;

        buf.put_u64(self.log_value_index);
        len += 8;

        buf.put_u32(self.map_index);
        len += 4;

        len
    }

    fn from_compact(mut buf: &[u8], _len: usize) -> (Self, &[u8]) {
        use bytes::Buf;

        let log_value_index = buf.get_u64();
        let map_index = buf.get_u32();

        (Self { log_value_index, map_index }, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_codecs::Compact;

    #[test]
    fn filter_maps_state_roundtrip() {
        let state = FilterMapsState { log_value_index: 1, map_index: 2 };
        let mut buf = vec![];
        let len = state.to_compact(&mut buf);
        let (decoded, _) = FilterMapsState::from_compact(&buf, len);
        assert_eq!(state, decoded);
    }
}
