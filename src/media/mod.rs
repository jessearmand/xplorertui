//! X photos: download, prepare for Jev and embedding models, and cache.

pub mod cache;
pub mod prepare;

pub use cache::{PhotoCache, PreparedPhoto};
