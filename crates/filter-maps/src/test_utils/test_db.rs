//! Test database utilities for filter maps

use crate::params::FilterMapParams;
use std::sync::Arc;

/// Test database for filter maps
#[derive(Debug)]
pub struct TestFilterMapDB {
    /// Filter map parameters
    pub params: Arc<FilterMapParams>,
}

impl TestFilterMapDB {
    /// Create a new test database with default parameters
    pub fn new() -> Self {
        Self { params: Arc::new(FilterMapParams::default()) }
    }

    /// Create a new test database with custom parameters
    pub fn with_params(params: FilterMapParams) -> Self {
        Self { params: Arc::new(params) }
    }
}

impl Default for TestFilterMapDB {
    fn default() -> Self {
        Self::new()
    }
}