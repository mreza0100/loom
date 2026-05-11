use loom_core::config::LoomConfig;
use loom_core::models::Symbol;
use loom_core::store::LoomDb;
use loom_core::LoomError;
use std::fs;

fn symbol(name: &str) -> Symbol {
    Symbol {
        id: None,
        name: name.to_string(),
        kind: "function".to_string(),
        file: "src/quote.rs".to_string(),
        line: 1,
        end_line: 1,
        language: "rust".to_string(),
        context: format!("fn {name}() {{}}"),
    }
}

#[test]
fn qa_config_rejects_non_finite_coupling_weights() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".loom")).unwrap();
    fs::write(
        temp.path().join(".loom/config.toml"),
        "structural_weight = nan",
    )
    .unwrap();

    assert!(matches!(
        LoomConfig::load(temp.path()),
        Err(LoomError::InvalidConfig(_))
    ));
}

#[test]
fn qa_config_rejects_absolute_db_path_escape() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".loom")).unwrap();
    fs::write(
        temp.path().join(".loom/config.toml"),
        r#"db_path = "/tmp/loom-escape.db""#,
    )
    .unwrap();

    assert!(matches!(
        LoomConfig::load(temp.path()),
        Err(LoomError::InvalidConfig(_))
    ));
}

#[test]
fn qa_fts_query_with_literal_quote_returns_gracefully() {
    let temp = tempfile::tempdir().unwrap();
    let config = LoomConfig::default_for_target(temp.path());
    let db = LoomDb::open(config).unwrap();
    db.insert_symbol(&symbol("quote\"bomb")).unwrap();

    let results = db.search_fts("quote\"bomb", 10);

    assert!(
        results.is_ok(),
        "literal quote in FTS input should not surface a SQLite syntax error"
    );
}
