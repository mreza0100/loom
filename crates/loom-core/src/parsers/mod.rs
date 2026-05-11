pub mod csharp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod parser;
pub mod python;
pub mod registry;
pub mod rust;
pub mod tree_sitter_utils;

use std::collections::BTreeSet;

use crate::{
    models::{ParsedEdge, Symbol},
    Result,
};

pub use parser::parse_file;
pub use registry::AdapterRegistry;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParseResult {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<ParsedEdge>,
}

pub trait LanguageAdapter: Send + Sync {
    fn extensions(&self) -> &'static [&'static str];
    fn language_name(&self) -> &'static str;
    fn excluded_dirs(&self) -> &'static [&'static str];
    fn parse(&self, source: &[u8], file_path: &str) -> Result<ParseResult>;
    fn resolve_module_path(
        &self,
        import_path: &str,
        source_file: &str,
        known_files: &BTreeSet<String>,
    ) -> String;
}
