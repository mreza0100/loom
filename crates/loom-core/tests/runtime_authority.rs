use loom_core::store::migrations::CURRENT_SCHEMA_VERSION;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("loom-core is under crates/loom-core")
        .to_path_buf()
}

fn collect_files(root: &Path, rel: &str, out: &mut Vec<PathBuf>) {
    let path = root.join(rel);
    if !path.exists() {
        return;
    }
    if path.is_file() {
        out.push(path);
        return;
    }

    let entries = fs::read_dir(&path).expect("active runtime directory is readable");
    for entry in entries {
        let entry = entry.expect("active runtime directory entry is readable");
        let path = entry.path();
        if path.is_dir() {
            if let Ok(nested) = path.strip_prefix(root) {
                collect_files(root, &nested.to_string_lossy(), out);
            }
        } else {
            out.push(path);
        }
    }
}

fn is_scannable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("json" | "md" | "py" | "sh" | "toml")
    )
}

#[test]
fn active_runtime_surfaces_do_not_reference_retired_python_runtime() {
    let root = workspace_root();
    let mut files = Vec::new();
    for rel in [
        ".claude/agents",
        ".claude/commands",
        ".codex/agents",
        ".codex/skills",
        "README.md",
        "INSTALL.md",
        "docs/dev/runtime-contract.md",
        "tmp/benchmark/README.md",
        "tmp/benchmark/scripts",
    ] {
        collect_files(&root, rel, &mut files);
    }

    let retired_runtime_patterns = [
        "python -m loom",
        "\"python\", \"-m\", \"loom\"",
        "'python', '-m', 'loom'",
        "uv run python tmp/benchmark/scripts/index-cockroach.py",
    ];
    let retired_db_patterns = [".loom.db"];
    let mut failures = Vec::new();

    for path in files.into_iter().filter(|path| is_scannable(path)) {
        let content = fs::read_to_string(&path).expect("active runtime file is UTF-8");
        for pattern in retired_runtime_patterns {
            if content.contains(pattern) {
                failures.push(format!("{} contains {pattern}", path.display()));
            }
        }
        for pattern in retired_db_patterns {
            if content.contains(pattern) {
                failures.push(format!(
                    "{} contains legacy store path {pattern}",
                    path.display()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "retired Python runtime references found in active surfaces:\n{}",
        failures.join("\n")
    );
}

#[test]
fn rust_runtime_contract_documents_active_semantics() {
    let root = workspace_root();
    let contract_path = root.join("docs/dev/runtime-contract.md");
    let contract = fs::read_to_string(&contract_path).expect("runtime contract exists");

    for required in [
        "target/debug/loom-mcp",
        ".loom/loom.db",
        "search",
        "related",
        "impact",
        "neighborhood",
        "schema_version",
        "embedder_backend",
        "embedder_degraded",
        "structural_weight = 0.45",
        "semantic_weight = 0.35",
        "evolutionary_weight = 0.20",
        "useful symbols per token",
    ] {
        assert!(
            contract.contains(required),
            "runtime contract missing required term: {required}"
        );
    }

    assert!(
        contract.contains(&format!(
            "CURRENT_SCHEMA_VERSION = {CURRENT_SCHEMA_VERSION}"
        )),
        "runtime contract must name the current schema version"
    );
}
