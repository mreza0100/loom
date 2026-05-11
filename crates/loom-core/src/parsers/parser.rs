use std::path::Path;

use crate::{
    error::{LoomError, Result},
    parsers::{signals::augment_parse_result, AdapterRegistry, ParseResult},
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
    let path_string = path.to_string_lossy();
    let bytes;
    let source = match source {
        Some(bytes) => bytes,
        None => {
            bytes = std::fs::read(path).map_err(|source| LoomError::ParserIo {
                path: path_string.clone().into_owned(),
                source,
            })?;
            &bytes
        }
    };

    let mut result = if let Some(adapter) = registry.get_adapter(&extension) {
        adapter.parse(source, &path_string)?
    } else {
        ParseResult::default()
    };
    augment_parse_result(&mut result, source, &path_string);
    Ok(result)
}
