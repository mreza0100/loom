use std::{collections::BTreeSet, fs};

use loom_core::{parsers::parse_file, AdapterRegistry, ParseResult};
use tempfile::tempdir;

fn names(result: &ParseResult) -> BTreeSet<String> {
    result
        .symbols
        .iter()
        .map(|symbol| symbol.name.clone())
        .collect()
}

fn has_edge(result: &ParseResult, source: &str, target: &str, relationship: &str) -> bool {
    result.edges.iter().any(|edge| {
        edge.source_name == source
            && edge.target_name == target
            && edge.relationship == relationship
    })
}

#[test]
fn registry_and_dispatcher_cover_builtin_extensions() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let extensions = registry.get_all_extensions();
    for extension in [
        ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".go", ".java", ".rs", ".cs",
    ] {
        assert!(
            extensions.contains(extension),
            "{extension} should be registered"
        );
        assert!(registry.get_adapter(extension).is_some());
    }
    assert!(registry.get_adapter(".txt").is_none());

    let excluded = registry.get_all_excluded_dirs();
    for directory in ["node_modules", "vendor", "target", "bin"] {
        assert!(excluded.contains(directory));
    }

    let unknown = parse_file(
        std::path::Path::new("example.txt"),
        Some(b"fn nope() {}"),
        &registry,
    )
    .expect("unknown extension should not fail");
    assert!(unknown.symbols.is_empty());
    assert!(unknown.edges.is_empty());
}

#[test]
fn dispatcher_reads_source_from_disk_when_needed() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("sample.ts");
    fs::write(&path, "function loaded() {\n  return 1;\n}\n").expect("write fixture");

    let result = parse_file(&path, None, &registry).expect("parse");

    assert!(names(&result).contains("loaded"));
}

#[test]
fn javascript_typescript_extract_symbols_imports_and_full_calls() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let source = br#"
import Widget, { getProduct as fetchProduct } from "./product.js";
const loader = require("./loader");
export function run() {
  this.hooks.make.callAsync();
  console.log("skip");
  new Widget();
}
class Service extends Base implements Runnable {
  start() { db.query(); }
}
type Alias = string;
interface Contract extends Parent {}
"#;

    let result =
        parse_file(std::path::Path::new("sample.ts"), Some(source), &registry).expect("parse");

    let symbol_names = names(&result);
    for expected in ["run", "Service", "Service.start", "Alias", "Contract"] {
        assert!(symbol_names.contains(expected), "missing {expected}");
    }
    assert!(has_edge(&result, "fetchProduct", "getProduct", "imports"));
    assert!(has_edge(
        &result,
        "run",
        "this.hooks.make.callAsync",
        "calls"
    ));
    assert!(!result
        .edges
        .iter()
        .any(|edge| edge.target_name == "console.log" && edge.relationship == "calls"));
    assert!(has_edge(&result, "run", "Widget", "instantiates"));
    assert!(has_edge(&result, "Service", "Base", "extends"));
}

#[test]
fn go_java_rust_and_csharp_smoke_extract_core_shapes() {
    let registry = AdapterRegistry::with_builtin_adapters();

    let go = parse_file(
        std::path::Path::new("sample.go"),
        Some(
            br#"
package main
import ("fmt"; "example.com/base")
type Base struct {}
type Service struct { Base }
func Run() { fmt.Println("x") }
func (s *Service) Start() { go Run() }
"#,
        ),
        &registry,
    )
    .expect("go parse");
    for expected in ["Base", "Service", "Run", "Service.Start"] {
        assert!(names(&go).contains(expected), "missing go {expected}");
    }
    assert!(has_edge(&go, "Service.Start", "Run", "calls"));

    let java = parse_file(
        std::path::Path::new("Sample.java"),
        Some(
            br#"
import java.util.List;
class Service extends Base implements Runnable {
  private int count;
  Service() { new Widget(); }
  void run() { helper.call(); }
  class Inner { void nested() {} }
}
"#,
        ),
        &registry,
    )
    .expect("java parse");
    for expected in [
        "Service",
        "Service.run",
        "Service.count",
        "Service.Inner",
        "Service.Inner.nested",
    ] {
        assert!(names(&java).contains(expected), "missing java {expected}");
    }
    assert!(has_edge(&java, "Service", "Base", "extends"));
    assert!(has_edge(&java, "Service.run", "helper.call", "calls"));

    let rust = parse_file(
        std::path::Path::new("lib.rs"),
        Some(
            br#"
use crate::foo::{Bar, Baz as Renamed};
const LIMIT: usize = 1;
struct Service;
enum Mode { Fast }
trait Runner { fn run(&self); }
impl Runner for Service { fn run(&self) { work!(); helper(); } }
macro_rules! work { () => {} }
"#,
        ),
        &registry,
    )
    .expect("rust parse");
    for expected in [
        "LIMIT",
        "Service",
        "Mode",
        "Mode.Fast",
        "Runner",
        "Service.run",
        "work",
    ] {
        assert!(names(&rust).contains(expected), "missing rust {expected}");
    }
    assert!(has_edge(&rust, "Service", "Runner", "implements"));
    assert!(has_edge(&rust, "Service.run", "helper", "calls"));

    let csharp = parse_file(
        std::path::Path::new("Sample.cs"),
        Some(
            br#"
using Alias = System.Text.StringBuilder;
namespace Demo {
  partial class Service : Base, IDisposable {
    private int Count;
    public string Name { get; set; }
    public Service() { new Widget(); }
    public void Run() { helper.Call(); }
  }
}
"#,
        ),
        &registry,
    )
    .expect("csharp parse");
    for expected in ["Service", "Service.Count", "Service.Name", "Service.Run"] {
        assert!(
            names(&csharp).contains(expected),
            "missing csharp {expected}"
        );
    }
    assert!(has_edge(&csharp, "Service", "Base", "extends"));
    assert!(has_edge(&csharp, "Service.Run", "helper.Call", "calls"));
}

#[test]
fn adapter_module_resolution_is_deterministic() {
    let registry = AdapterRegistry::with_builtin_adapters();
    let known = BTreeSet::from([
        "src/product.ts".to_string(),
        "foo/bar.rs".to_string(),
        "foo/bar/mod.rs".to_string(),
        "com/example/Foo.java".to_string(),
        "util/file.go".to_string(),
    ]);

    let js = registry.get_adapter(".ts").expect("ts adapter");
    assert_eq!(
        js.resolve_module_path("src/product", "src/main.ts", &known),
        "src/product.ts"
    );

    let rust = registry.get_adapter(".rs").expect("rust adapter");
    assert_eq!(
        rust.resolve_module_path("crate::foo::bar", "src/lib.rs", &known),
        "foo/bar.rs"
    );

    let java = registry.get_adapter(".java").expect("java adapter");
    assert_eq!(
        java.resolve_module_path("com.example.Foo", "src/Sample.java", &known),
        "com/example/Foo.java"
    );
}
