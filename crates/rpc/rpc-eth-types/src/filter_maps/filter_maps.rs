use crate::filter_maps::FilterMapParams;

/// A full or partial in-memory representation of a filter map where rows are
/// allowed to have a nil value meaning the row is not stored in the structure.
/// Note that therefore a known empty row should be represented with a zero-
/// length slice. It can be used as a memory cache or an overlay while preparing
/// a batch of changes to the structure. In either case a nil value should be
/// interpreted as transparent (uncached/unchanged).
pub type FilterMap = Vec<FilterRow>;

/// FilterRow encodes a single row of a filter map as a list of column indices.
/// Note that the values are always stored in the same order as they were added
/// and if the same column index is added twice, it is also stored twice.
/// Order of column indices and potential duplications do not matter when
/// searching for a value but leaving the original order makes reverting to a
/// previous state simpler.
pub type FilterRow = Vec<u32>;

/// FilterMaps is a collection of filter maps.
pub struct FilterMaps {
    pub params: FilterMapParams,
    pub filter_maps: Vec<FilterMap>,
}
