use std::collections::{BTreeMap, BTreeSet};

use super::{
    csharp::CSharpAdapter, go::GoAdapter, java::JavaAdapter, javascript::JavaScriptAdapter,
    python::PythonAdapter, rust::RustAdapter, LanguageAdapter,
};

pub struct AdapterRegistry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
    by_extension: BTreeMap<String, usize>,
}

impl AdapterRegistry {
    #[must_use]
    pub fn with_builtin_adapters() -> Self {
        let mut registry = Self {
            adapters: Vec::new(),
            by_extension: BTreeMap::new(),
        };
        registry.register(Box::new(JavaScriptAdapter));
        registry.register(Box::new(PythonAdapter));
        registry.register(Box::new(GoAdapter));
        registry.register(Box::new(JavaAdapter));
        registry.register(Box::new(RustAdapter));
        registry.register(Box::new(CSharpAdapter));
        registry
    }

    pub fn register(&mut self, adapter: Box<dyn LanguageAdapter>) {
        let index = self.adapters.len();
        for extension in adapter.extensions() {
            self.by_extension.insert((*extension).to_string(), index);
        }
        self.adapters.push(adapter);
    }

    #[must_use]
    pub fn get_adapter(&self, extension: &str) -> Option<&dyn LanguageAdapter> {
        self.by_extension
            .get(extension)
            .and_then(|index| self.adapters.get(*index).map(Box::as_ref))
    }

    #[must_use]
    pub fn get_all_extensions(&self) -> BTreeSet<String> {
        self.by_extension.keys().cloned().collect()
    }

    #[must_use]
    pub fn get_all_excluded_dirs(&self) -> BTreeSet<String> {
        self.adapters
            .iter()
            .flat_map(|adapter| adapter.excluded_dirs().iter().copied())
            .map(str::to_string)
            .collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::with_builtin_adapters()
    }
}
