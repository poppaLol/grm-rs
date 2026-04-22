use crate::error::Result;
use std::path::Path;

/// Trait for types that support graph persistence to/from files
pub trait GraphPersistence: Sized {
    /// Save the graph to a file in JSON format
    fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()>;

    /// Load a graph from a file in JSON format
    fn load_from_file(path: impl AsRef<Path>) -> Result<Self>;

    /// Save the graph to a file in a compact binary format.
    fn save_to_binary_file(&self, path: impl AsRef<Path>) -> Result<()>;

    /// Load a graph from a file in a compact binary format.
    fn load_from_binary_file(path: impl AsRef<Path>) -> Result<Self>;
}
