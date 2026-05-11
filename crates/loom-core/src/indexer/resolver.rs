use crate::{
    models::{Edge, Symbol},
    parsers::AdapterRegistry,
    store::LoomDb,
    Result,
};
use std::collections::{BTreeMap, BTreeSet};

type ImportMap = BTreeMap<(String, String), (String, Option<String>)>;

pub struct EdgeResolver<'a> {
    db: &'a LoomDb,
    registry: AdapterRegistry,
}

impl<'a> EdgeResolver<'a> {
    pub fn new(db: &'a LoomDb) -> Self {
        Self {
            db,
            registry: AdapterRegistry::with_builtin_adapters(),
        }
    }

    pub fn resolve_all(&self) -> Result<usize> {
        let import_map = self.build_import_map()?;
        let unresolved = self.db.get_unresolved_edges()?;
        let mut resolutions = Vec::new();
        for edge in unresolved {
            if let Some((target_id, confidence)) = self.resolve_single_edge(&edge, &import_map)? {
                if let Some(edge_id) = edge.id {
                    resolutions.push((edge_id, target_id, confidence));
                }
            }
        }
        self.db.resolve_edges_batch(&resolutions)?;
        Ok(resolutions.len())
    }

    fn build_import_map(&self) -> Result<ImportMap> {
        let known_files: BTreeSet<String> = self.db.list_symbol_files()?.into_iter().collect();
        let mut import_map = BTreeMap::new();
        for row in self.db.get_import_edges_with_source_file()? {
            let resolved =
                self.resolve_module_file(&row.target_file, &known_files, &row.source_file);
            import_map.insert(
                (row.source_file, row.local_name),
                (resolved, row.original_name),
            );
        }
        Ok(import_map)
    }

    fn resolve_module_file(
        &self,
        target_file: &str,
        known_files: &BTreeSet<String>,
        source_file: &str,
    ) -> String {
        let extension = std::path::Path::new(source_file)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!(".{extension}"));
        extension
            .as_deref()
            .and_then(|extension| self.registry.get_adapter(extension))
            .map(|adapter| adapter.resolve_module_path(target_file, source_file, known_files))
            .unwrap_or_else(|| target_file.to_string())
    }

    fn resolve_single_edge(
        &self,
        edge: &Edge,
        import_map: &ImportMap,
    ) -> Result<Option<(i64, f64)>> {
        if let Some(target_file) = edge.target_file.as_deref() {
            if let Some(symbol_id) = unique_id(
                self.db
                    .get_symbol_by_name(&edge.target_name, Some(target_file))?,
            ) {
                return Ok(Some((symbol_id, 1.0)));
            }
        }

        let Some(source_symbol) = self.db.get_symbol_by_id(edge.source_id)? else {
            return Ok(None);
        };
        let source_file = source_symbol.file.as_str();
        let parts = edge.target_name.split('.').collect::<Vec<_>>();
        let Some(base) = parts.first().copied() else {
            return Ok(None);
        };

        if let Some((resolved_file, original_name)) =
            import_map.get(&(source_file.to_string(), base.to_string()))
        {
            if parts.len() == 1 {
                if let Some(symbol_id) = unique_id(
                    self.db
                        .get_symbol_by_name(&edge.target_name, Some(resolved_file))?,
                ) {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(original_name) = original_name {
                    if original_name != &edge.target_name {
                        if let Some(symbol_id) = unique_id(
                            self.db
                                .get_symbol_by_name(original_name, Some(resolved_file))?,
                        ) {
                            return Ok(Some((symbol_id, 0.95)));
                        }
                    }
                }
            } else {
                let method = parts[1..].join(".");
                let direct = self.db.get_symbol_by_name(&method, Some(resolved_file))?;
                if let Some(symbol_id) = unique_id(direct) {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(symbol_id) = unique_id(
                    self.db
                        .get_symbol_by_name(&format!("{base}.{method}"), Some(resolved_file))?,
                ) {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(original_name) = original_name {
                    if let Some(symbol_id) = unique_id(self.db.get_symbol_by_name(
                        &format!("{original_name}.{method}"),
                        Some(resolved_file),
                    )?) {
                        return Ok(Some((symbol_id, 0.95)));
                    }
                }
            }
        }

        if base == "this" && parts.len() >= 2 {
            let method = parts[1..].join(".");
            if let Some(class_prefix) = source_symbol.name.split_once('.').map(|(prefix, _)| prefix)
            {
                let qualified = format!("{class_prefix}.{method}");
                if let Some(symbol_id) =
                    unique_id(self.db.get_symbol_by_name(&qualified, Some(source_file))?)
                {
                    return Ok(Some((symbol_id, 0.95)));
                }
            }
            let pattern = format!("%.{}", method);
            if let Some(symbol_id) = unique_id(self.db.find_symbols_like_name(
                &pattern,
                Some(source_file),
                20,
            )?) {
                return Ok(Some((symbol_id, 0.9)));
            }
        }

        if let Some(target_file) = edge.target_file.as_deref() {
            if !import_map.contains_key(&(source_file.to_string(), base.to_string())) {
                let suffix = target_file.trim_start_matches("./");
                let slash_suffix = format!("/{suffix}");
                let matches = self
                    .db
                    .get_symbol_by_name(&edge.target_name, None)?
                    .into_iter()
                    .filter(|symbol| {
                        symbol.file.ends_with(suffix) || symbol.file.ends_with(&slash_suffix)
                    })
                    .collect::<Vec<_>>();
                if let Some(symbol_id) = unique_id(matches) {
                    return Ok(Some((symbol_id, 0.9)));
                }
            }
        }

        let simple_name = parts.last().copied().unwrap_or(&edge.target_name);
        if simple_name != edge.target_name {
            if let Some(symbol_id) = unique_id(self.db.get_symbol_by_name(&edge.target_name, None)?)
            {
                if parts[0].chars().next().is_some_and(char::is_uppercase) {
                    return Ok(Some((symbol_id, 1.0)));
                }
                return Ok(Some((symbol_id, 0.8)));
            }
        }

        let pattern = format!("%.{}", simple_name);
        if let Some(symbol_id) = unique_id(self.db.find_symbols_like_name(&pattern, None, 20)?) {
            return Ok(Some((symbol_id, 0.8)));
        }

        if let Some(symbol_id) = unique_id(self.db.get_symbol_by_name(simple_name, None)?) {
            return Ok(Some((symbol_id, 0.6)));
        }

        Ok(None)
    }
}

fn unique_id(symbols: Vec<Symbol>) -> Option<i64> {
    if symbols.len() == 1 {
        symbols[0].id
    } else {
        None
    }
}
