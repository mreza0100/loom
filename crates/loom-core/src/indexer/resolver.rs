use crate::{
    models::{Edge, Symbol},
    parsers::AdapterRegistry,
    store::LoomDb,
    Result,
};
use std::collections::{BTreeMap, BTreeSet};

type ImportMap = BTreeMap<(String, String), (String, Option<String>)>;

#[derive(Debug, Default)]
struct ResolverIndex {
    symbols_by_id: BTreeMap<i64, Symbol>,
    exact_by_name: BTreeMap<String, Vec<i64>>,
    exact_by_file_name: BTreeMap<(String, String), Vec<i64>>,
    suffix_by_name: BTreeMap<String, Vec<i64>>,
    suffix_by_file_name: BTreeMap<(String, String), Vec<i64>>,
}

impl ResolverIndex {
    fn from_symbols(symbols: Vec<Symbol>) -> Self {
        let mut index = Self::default();
        for symbol in symbols {
            let Some(symbol_id) = symbol.id else {
                continue;
            };
            index
                .exact_by_name
                .entry(symbol.name.clone())
                .or_default()
                .push(symbol_id);
            index
                .exact_by_file_name
                .entry((symbol.file.clone(), symbol.name.clone()))
                .or_default()
                .push(symbol_id);
            if let Some((_, suffix)) = symbol.name.rsplit_once('.') {
                index
                    .suffix_by_name
                    .entry(suffix.to_string())
                    .or_default()
                    .push(symbol_id);
                index
                    .suffix_by_file_name
                    .entry((symbol.file.clone(), suffix.to_string()))
                    .or_default()
                    .push(symbol_id);
            }
            index.symbols_by_id.insert(symbol_id, symbol);
        }
        index
    }

    fn source_symbol(&self, symbol_id: i64) -> Option<&Symbol> {
        self.symbols_by_id.get(&symbol_id)
    }

    fn unique_exact(&self, name: &str, file: Option<&str>) -> Option<i64> {
        let ids = if let Some(file) = file {
            self.exact_by_file_name
                .get(&(file.to_string(), name.to_string()))
        } else {
            self.exact_by_name.get(name)
        }?;
        unique_id_slice(ids)
    }

    fn unique_suffix(&self, suffix: &str, file: Option<&str>) -> Option<i64> {
        let ids = if let Some(file) = file {
            self.suffix_by_file_name
                .get(&(file.to_string(), suffix.to_string()))
        } else {
            self.suffix_by_name.get(suffix)
        }?;
        unique_id_slice(ids)
    }

    fn exact_symbols(&self, name: &str) -> Vec<&Symbol> {
        self.exact_by_name
            .get(name)
            .into_iter()
            .flat_map(|ids| ids.iter())
            .filter_map(|id| self.symbols_by_id.get(id))
            .collect()
    }
}

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
        let symbol_index = ResolverIndex::from_symbols(self.db.list_all_symbols()?);
        let unresolved = self.db.get_unresolved_edges()?;
        let mut resolutions = Vec::new();
        for edge in unresolved {
            if let Some((target_id, confidence)) =
                self.resolve_single_edge(&edge, &import_map, &symbol_index)?
            {
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
        symbol_index: &ResolverIndex,
    ) -> Result<Option<(i64, f64)>> {
        if let Some(target_file) = edge.target_file.as_deref() {
            if let Some(symbol_id) = symbol_index.unique_exact(&edge.target_name, Some(target_file))
            {
                return Ok(Some((symbol_id, 1.0)));
            }
        }

        let Some(source_symbol) = symbol_index.source_symbol(edge.source_id) else {
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
                if let Some(symbol_id) =
                    symbol_index.unique_exact(&edge.target_name, Some(resolved_file))
                {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(original_name) = original_name {
                    if original_name != &edge.target_name {
                        if let Some(symbol_id) =
                            symbol_index.unique_exact(original_name, Some(resolved_file))
                        {
                            return Ok(Some((symbol_id, 0.95)));
                        }
                    }
                }
            } else {
                let method = parts[1..].join(".");
                if let Some(symbol_id) = symbol_index.unique_exact(&method, Some(resolved_file)) {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(symbol_id) =
                    symbol_index.unique_exact(&format!("{base}.{method}"), Some(resolved_file))
                {
                    return Ok(Some((symbol_id, 0.95)));
                }
                if let Some(original_name) = original_name {
                    if let Some(symbol_id) = symbol_index
                        .unique_exact(&format!("{original_name}.{method}"), Some(resolved_file))
                    {
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
                if let Some(symbol_id) = symbol_index.unique_exact(&qualified, Some(source_file)) {
                    return Ok(Some((symbol_id, 0.95)));
                }
            }
            if let Some(symbol_id) = symbol_index.unique_suffix(&method, Some(source_file)) {
                return Ok(Some((symbol_id, 0.9)));
            }
        }

        if let Some(target_file) = edge.target_file.as_deref() {
            if !import_map.contains_key(&(source_file.to_string(), base.to_string())) {
                let suffix = target_file.trim_start_matches("./");
                let slash_suffix = format!("/{suffix}");
                let matches = symbol_index
                    .exact_symbols(&edge.target_name)
                    .into_iter()
                    .filter(|symbol| {
                        symbol.file.ends_with(suffix) || symbol.file.ends_with(&slash_suffix)
                    })
                    .filter_map(|symbol| symbol.id)
                    .collect::<Vec<_>>();
                if let Some(symbol_id) = unique_id_slice(&matches) {
                    return Ok(Some((symbol_id, 0.9)));
                }
            }
        }

        let simple_name = parts.last().copied().unwrap_or(&edge.target_name);
        if simple_name != edge.target_name {
            if let Some(symbol_id) = symbol_index.unique_exact(&edge.target_name, None) {
                if parts[0].chars().next().is_some_and(char::is_uppercase) {
                    return Ok(Some((symbol_id, 1.0)));
                }
                return Ok(Some((symbol_id, 0.8)));
            }
        }

        if let Some(symbol_id) = symbol_index.unique_suffix(simple_name, None) {
            return Ok(Some((symbol_id, 0.8)));
        }

        if let Some(symbol_id) = symbol_index.unique_exact(simple_name, None) {
            return Ok(Some((symbol_id, 0.6)));
        }

        Ok(None)
    }
}

fn unique_id_slice(symbol_ids: &[i64]) -> Option<i64> {
    if symbol_ids.len() == 1 {
        Some(symbol_ids[0])
    } else {
        None
    }
}
