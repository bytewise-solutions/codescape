pub mod parse;
mod resolve;

pub use resolve::{extract_go, is_mock_dir, is_mock_type};
