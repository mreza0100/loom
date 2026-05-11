use std::path::Path;

use crate::{
    error::{LoomError, Result},
    parsers::{AdapterRegistry, ParseResult},
};

pub fn parse_file(
    path: &Path,
    source: Option<&[u8]>,
    registry: &AdapterRegistry,
) -> Result<ParseResult> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"));

    let Some(extension) = extension else {
        return Ok(ParseResult::default());
    };
    let Some(adapter) = registry.get_adapter(&extension) else {
        return Ok(ParseResult::default());
    };

    match source {
        Some(bytes) => adapter.parse(bytes, &path.to_string_lossy()),
        None => {
            let bytes = std::fs::read(path).map_err(|source| LoomError::ParserIo {
                path: path.to_string_lossy().into_owned(),
                source,
            })?;
            adapter.parse(&bytes, &path.to_string_lossy())
        }
    }
}
