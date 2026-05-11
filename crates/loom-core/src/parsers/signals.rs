use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    models::{ParsedAlias, ParsedBehaviorFact, ParsedCallsite},
    parsers::ParseResult,
};

const SECRET_MARKERS: [&str; 11] = [
    "secret",
    "password",
    "passwd",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "authorization",
    "credential",
    "bearer",
];

const CONTROL_CALLEES: [&str; 12] = [
    "if", "for", "while", "switch", "catch", "match", "return", "sizeof", "typeof", "await",
    "loop", "unsafe",
];

const GENERIC_CALLEES: [&str; 20] = [
    "log",
    "println",
    "print",
    "debug",
    "info",
    "warn",
    "error",
    "map",
    "filter",
    "then",
    "catch",
    "unwrap",
    "expect",
    "clone",
    "to_string",
    "to_owned",
    "get",
    "set",
    "push",
    "insert",
];

pub fn augment_parse_result(result: &mut ParseResult, source: &[u8], file_path: &str) {
    let text = String::from_utf8_lossy(source);
    let symbol_spans = result
        .symbols
        .iter()
        .map(|symbol| (symbol.line, symbol.end_line, symbol.name.clone()))
        .collect::<Vec<_>>();
    let lines = text.lines().collect::<Vec<_>>();

    extract_aliases(result, &lines);
    let alias_map = result
        .aliases
        .iter()
        .map(|alias| (alias.local_name.clone(), alias.source.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut in_package_scripts = false;
    let manifest_kind = manifest_kind(file_path);
    for (index, line) in lines.iter().enumerate() {
        let line_no = i64::try_from(index + 1).unwrap_or(i64::MAX);
        let enclosing_symbol_name = enclosing_symbol_name(line_no, &symbol_spans);
        let literals = string_literals(line);
        if !looks_secret(line) {
            extract_behavior_facts(
                result,
                line,
                &literals,
                line_no,
                enclosing_symbol_name.clone(),
                manifest_kind,
                &mut in_package_scripts,
            );
        }
        extract_callsites(
            result,
            line,
            line_no,
            enclosing_symbol_name,
            &alias_map,
            looks_secret(line),
        );
    }

    dedupe_signals(result);
}

fn extract_behavior_facts(
    result: &mut ParseResult,
    line: &str,
    literals: &[String],
    line_no: i64,
    enclosing_symbol_name: Option<String>,
    manifest_kind: Option<&str>,
    in_package_scripts: &mut bool,
) {
    extract_env_fact(result, line, line_no, enclosing_symbol_name.clone());
    extract_package_or_script_facts(
        result,
        line,
        line_no,
        enclosing_symbol_name.clone(),
        manifest_kind,
        in_package_scripts,
    );

    let lower = line.to_ascii_lowercase();
    for literal in literals {
        if literal.trim().is_empty() || looks_secret(literal) {
            continue;
        }
        if is_config_path(literal) {
            push_fact(
                result,
                "config_path",
                literal,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
        if looks_error_line(&lower) && is_useful_string(literal) {
            push_fact(
                result,
                "error_string",
                literal,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
        if looks_feature_flag_line(&lower) && is_useful_string(literal) {
            push_fact(
                result,
                "feature_flag",
                literal,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
        if looks_command_line(&lower) && is_useful_string(literal) {
            push_fact(
                result,
                "command",
                command_name(literal),
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
        if manifest_kind.is_some() && is_path_like(literal) {
            push_fact(
                result,
                "manifest_path",
                literal,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
        if is_important_literal(literal, &lower) {
            push_fact(
                result,
                "important_string",
                literal,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
    }
}

fn extract_env_fact(
    result: &mut ParseResult,
    line: &str,
    line_no: i64,
    enclosing_symbol_name: Option<String>,
) {
    for marker in ["process.env.", "import.meta.env."] {
        for value in values_after_marker(line, marker) {
            push_fact(
                result,
                "environment_variable",
                &value,
                line_no,
                enclosing_symbol_name.clone(),
            );
        }
    }
    for marker in [
        "env::var(",
        "std::env::var(",
        "System.getenv(",
        "os.Getenv(",
        "Deno.env.get(",
        "Environment.GetEnvironmentVariable(",
    ] {
        if let Some(after) = line.split(marker).nth(1) {
            if let Some(value) = string_literals(after).first() {
                push_fact(
                    result,
                    "environment_variable",
                    value,
                    line_no,
                    enclosing_symbol_name.clone(),
                );
            }
        }
    }
}

fn extract_package_or_script_facts(
    result: &mut ParseResult,
    line: &str,
    line_no: i64,
    enclosing_symbol_name: Option<String>,
    manifest_kind: Option<&str>,
    in_package_scripts: &mut bool,
) {
    let Some(kind) = manifest_kind else {
        return;
    };
    let trimmed = line.trim();
    if kind == "package_json" {
        if trimmed.starts_with("\"scripts\"") {
            *in_package_scripts = true;
        } else if *in_package_scripts && trimmed.starts_with('}') {
            *in_package_scripts = false;
        }

        if let Some((key, value)) = quoted_key_value(trimmed) {
            if key == "name" {
                push_fact(
                    result,
                    "package_name",
                    &value,
                    line_no,
                    enclosing_symbol_name.clone(),
                );
            } else if *in_package_scripts {
                push_fact(
                    result,
                    "script_name",
                    &key,
                    line_no,
                    enclosing_symbol_name.clone(),
                );
                push_fact(
                    result,
                    "command",
                    command_name(&value),
                    line_no,
                    enclosing_symbol_name.clone(),
                );
            }
        }
    }

    if kind == "toml" {
        if let Some((key, value)) = bare_key_value(trimmed) {
            if key == "name" {
                push_fact(
                    result,
                    "package_name",
                    &value,
                    line_no,
                    enclosing_symbol_name.clone(),
                );
            }
        }
    }
}

fn extract_callsites(
    result: &mut ParseResult,
    line: &str,
    line_no: i64,
    enclosing_symbol_name: Option<String>,
    alias_map: &BTreeMap<String, String>,
    redact_strings: bool,
) {
    let mut search_start = 0usize;
    while let Some(relative) = line[search_start..].find('(') {
        let open = search_start + relative;
        let Some((target_start, target)) = call_target_before(line, open) else {
            search_start = open + 1;
            continue;
        };
        if should_skip_call(line, target_start, &target) {
            search_start = open + 1;
            continue;
        }
        let Some(close) = matching_close_paren(line, open) else {
            search_start = open + 1;
            continue;
        };
        let args = summarize_arguments(&line[open + 1..close], redact_strings);
        let (receiver, callee) = split_receiver_callee(&target);
        let base = receiver
            .as_deref()
            .and_then(|receiver| receiver.split(['.', ':']).next())
            .unwrap_or(&callee);
        let imported_aliases = alias_map
            .get(base)
            .map(|source| vec![format!("{base}->{source}")])
            .unwrap_or_default();
        let generic = is_generic_callee(&callee);
        let confidence =
            callsite_confidence(receiver.as_deref(), !imported_aliases.is_empty(), generic);
        result.callsites.push(ParsedCallsite {
            line: line_no,
            end_line: line_no,
            callee,
            receiver,
            unresolved_target: target,
            argument_summaries: args,
            imported_aliases,
            enclosing_symbol_name: enclosing_symbol_name.clone(),
            confidence,
            generic,
            downweighted: generic,
        });
        search_start = close + 1;
    }
}

fn extract_aliases(result: &mut ParseResult, lines: &[&str]) {
    for (index, line) in lines.iter().enumerate() {
        let line_no = i64::try_from(index + 1).unwrap_or(i64::MAX);
        let trimmed = line.trim();
        if trimmed.starts_with("import \"") || trimmed.starts_with("import (") {
            for literal in string_literals(trimmed) {
                let local = literal.rsplit('/').next().unwrap_or(&literal).to_string();
                push_alias(result, line_no, &local, &local, &literal, "go_import");
            }
        } else if trimmed.starts_with("import ") && trimmed.contains(" from ") {
            extract_js_import_aliases(result, trimmed, line_no);
        } else if trimmed.starts_with("import ") {
            let source = trimmed
                .trim_start_matches("import ")
                .trim_end_matches(';')
                .trim();
            if !source.is_empty() && !source.contains(' ') {
                let local = source.rsplit('.').next().unwrap_or(source);
                push_alias(result, line_no, local, local, source, "java_import");
            }
        } else if trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
        {
            extract_require_alias(result, trimmed, line_no);
        } else if trimmed.starts_with("use ") {
            extract_rust_use_aliases(result, trimmed, line_no);
        } else if trimmed.starts_with("using ") {
            extract_csharp_using_alias(result, trimmed, line_no);
        }
    }
}

fn extract_js_import_aliases(result: &mut ParseResult, line: &str, line_no: i64) {
    let Some(module) = string_literals(line).last().cloned() else {
        return;
    };
    let Some((raw_spec, _)) = line.split_once(" from ") else {
        if let Some(side_effect) = string_literals(line).first() {
            push_alias(
                result,
                line_no,
                side_effect,
                side_effect,
                side_effect,
                "side_effect",
            );
        }
        return;
    };
    let spec = raw_spec.trim_start_matches("import").trim();
    if let (Some(start), Some(end)) = (spec.find('{'), spec.rfind('}')) {
        let before = spec[..start].trim().trim_end_matches(',').trim();
        if !before.is_empty() && !before.starts_with('*') {
            push_alias(
                result,
                line_no,
                before,
                "default",
                &module,
                "default_import",
            );
        }
        for entry in spec[start + 1..end].split(',').map(str::trim) {
            if entry.is_empty() {
                continue;
            }
            let parts = entry.split_whitespace().collect::<Vec<_>>();
            match parts.as_slice() {
                [imported, "as", local] => {
                    push_alias(result, line_no, local, imported, &module, "named_import");
                }
                [local] => push_alias(result, line_no, local, local, &module, "named_import"),
                _ => {}
            }
        }
    } else if let Some(local) = spec.strip_prefix("* as ").map(str::trim) {
        push_alias(result, line_no, local, "*", &module, "namespace_import");
    } else if !spec.is_empty() {
        push_alias(result, line_no, spec, "default", &module, "default_import");
    }
}

fn extract_require_alias(result: &mut ParseResult, line: &str, line_no: i64) {
    if !line.contains("require(") {
        return;
    }
    let Some(module) = string_literals(line).first().cloned() else {
        return;
    };
    let Some((left, _)) = line.split_once('=') else {
        return;
    };
    let local = left
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim_matches(['{', '}', ',']);
    if !local.is_empty() {
        push_alias(result, line_no, local, local, &module, "require");
    }
}

fn extract_rust_use_aliases(result: &mut ParseResult, line: &str, line_no: i64) {
    let body = line.trim_start_matches("use ").trim_end_matches(';').trim();
    if let (Some(start), Some(end)) = (body.find('{'), body.rfind('}')) {
        let prefix = body[..start].trim_end_matches("::");
        for entry in body[start + 1..end].split(',').map(str::trim) {
            if entry.is_empty() {
                continue;
            }
            let (imported, local) = split_alias(entry, " as ");
            let source = if prefix.is_empty() {
                imported.to_string()
            } else {
                format!("{prefix}::{imported}")
            };
            push_alias(result, line_no, local, imported, &source, "rust_use");
        }
    } else {
        let (imported, local) = split_alias(body, " as ");
        push_alias(result, line_no, local, imported, body, "rust_use");
    }
}

fn extract_csharp_using_alias(result: &mut ParseResult, line: &str, line_no: i64) {
    let body = line
        .trim_start_matches("using ")
        .trim_end_matches(';')
        .trim();
    if let Some((local, source)) = body.split_once('=') {
        let local = local.trim();
        let source = source.trim();
        if !local.is_empty() && !source.is_empty() {
            push_alias(result, line_no, local, source, source, "using_alias");
        }
    }
}

fn split_alias<'a>(value: &'a str, separator: &str) -> (&'a str, &'a str) {
    if let Some((imported, local)) = value.split_once(separator) {
        (imported.trim(), local.trim())
    } else {
        let imported = value.trim();
        let local = imported.rsplit("::").next().unwrap_or(imported);
        (imported, local)
    }
}

fn push_fact(
    result: &mut ParseResult,
    fact_type: &str,
    value: &str,
    line: i64,
    enclosing_symbol_name: Option<String>,
) {
    let value = bounded_value(value.trim());
    if value.is_empty() {
        return;
    }
    result.behavior_facts.push(ParsedBehaviorFact {
        fact_type: fact_type.to_string(),
        value,
        line,
        end_line: line,
        enclosing_symbol_name,
    });
}

fn push_alias(
    result: &mut ParseResult,
    line: i64,
    local_name: &str,
    imported_name: &str,
    source: &str,
    alias_kind: &str,
) {
    if local_name.is_empty() || imported_name.is_empty() || source.is_empty() {
        return;
    }
    result.aliases.push(ParsedAlias {
        line,
        end_line: line,
        local_name: bounded_value(local_name),
        imported_name: bounded_value(imported_name),
        source: bounded_value(source),
        alias_kind: alias_kind.to_string(),
        enclosing_symbol_name: None,
    });
}

fn dedupe_signals(result: &mut ParseResult) {
    let mut facts = BTreeSet::new();
    result.behavior_facts.retain(|fact| {
        facts.insert((
            fact.fact_type.clone(),
            fact.value.clone(),
            fact.line,
            fact.enclosing_symbol_name.clone(),
        ))
    });

    let mut aliases = BTreeSet::new();
    result.aliases.retain(|alias| {
        aliases.insert((
            alias.local_name.clone(),
            alias.imported_name.clone(),
            alias.source.clone(),
            alias.line,
        ))
    });

    let mut callsites = BTreeSet::new();
    result.callsites.retain(|callsite| {
        callsites.insert((
            callsite.unresolved_target.clone(),
            callsite.line,
            callsite.enclosing_symbol_name.clone(),
        ))
    });
}

fn values_after_marker(line: &str, marker: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = line[offset..].find(marker) {
        let start = offset + found + marker.len();
        let value = line[start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect::<String>();
        if !value.is_empty() {
            values.push(value);
        }
        offset = start.saturating_add(1);
    }
    values
}

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((start, quote)) = chars.next() {
        if !matches!(quote, '"' | '\'' | '`') {
            continue;
        }
        let mut escaped = false;
        let mut end = None;
        for (index, character) in chars.by_ref() {
            if escaped {
                escaped = false;
                continue;
            }
            if character == '\\' {
                escaped = true;
                continue;
            }
            if character == quote {
                end = Some(index);
                break;
            }
        }
        if let Some(end) = end {
            literals.push(line[start + quote.len_utf8()..end].to_string());
        }
    }
    literals
}

fn quoted_key_value(line: &str) -> Option<(String, String)> {
    let literals = string_literals(line);
    if literals.len() >= 2 && line.contains(':') {
        Some((literals[0].clone(), literals[1].clone()))
    } else {
        None
    }
}

fn bare_key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    let value = string_literals(value).first().cloned()?;
    Some((key.to_string(), value))
}

fn enclosing_symbol_name(line: i64, symbol_spans: &[(i64, i64, String)]) -> Option<String> {
    symbol_spans
        .iter()
        .filter(|(start, end, _)| *start <= line && line <= *end)
        .min_by_key(|(start, end, _)| end.saturating_sub(*start))
        .map(|(_, _, name)| name.clone())
}

fn manifest_kind(file_path: &str) -> Option<&'static str> {
    let file_name = Path::new(file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if file_name == "package.json" {
        return Some("package_json");
    }
    match Path::new(file_path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("toml") => Some("toml"),
        Some("json" | "yaml" | "yml") => Some("manifest"),
        _ => None,
    }
}

fn looks_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    SECRET_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn looks_error_line(lower_line: &str) -> bool {
    [
        "throw ",
        "new error",
        "panic!",
        "anyhow!",
        "bail!",
        "errors.new",
        "fmt.errorf",
        "return err",
        "invalid",
    ]
    .iter()
    .any(|marker| lower_line.contains(marker))
}

fn looks_feature_flag_line(lower_line: &str) -> bool {
    ["feature", "flag", "isenabled", "launchdarkly"]
        .iter()
        .any(|marker| lower_line.contains(marker))
}

fn looks_command_line(lower_line: &str) -> bool {
    [
        "command::new",
        "cmd(",
        "exec(",
        "spawn(",
        "command(",
        "new command",
        "std::process::command",
    ]
    .iter()
    .any(|marker| lower_line.contains(marker))
}

fn is_config_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let ends_config = [".json", ".yaml", ".yml", ".toml"]
        .iter()
        .any(|suffix| lower.ends_with(suffix));
    ends_config && (lower.contains('/') || lower.contains("config") || lower.contains("package"))
}

fn is_path_like(value: &str) -> bool {
    value.contains('/')
        || [".json", ".yaml", ".yml", ".toml", ".rs", ".ts", ".js"]
            .iter()
            .any(|suffix| value.to_ascii_lowercase().ends_with(suffix))
}

fn is_useful_string(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() >= 2 && trimmed.len() <= 200 && trimmed.chars().any(char::is_alphabetic)
}

fn is_important_literal(value: &str, lower_line: &str) -> bool {
    if !is_useful_string(value)
        || lower_line.trim_start().starts_with("import ")
        || lower_line.contains("require(")
    {
        return false;
    }
    value.contains('/')
        || value.contains('.')
        || value.contains('_')
        || value.contains('-')
        || (value.len() >= 12 && value.split_whitespace().count() >= 2)
}

fn command_name(value: &str) -> &str {
    value.split_whitespace().next().unwrap_or(value)
}

fn bounded_value(value: &str) -> String {
    let mut output = value.chars().take(240).collect::<String>();
    if value.chars().count() > 240 {
        output.push_str("...");
    }
    output
}

fn call_target_before(line: &str, open: usize) -> Option<(usize, String)> {
    let before = &line[..open];
    let end = before.trim_end().len();
    if end == 0 {
        return None;
    }
    let mut start = end;
    for (index, character) in before[..end].char_indices().rev() {
        if is_target_char(character) {
            start = index;
        } else {
            break;
        }
    }
    let target = before[start..end].trim().trim_end_matches('!').to_string();
    if target.is_empty() {
        None
    } else {
        Some((start, target))
    }
}

fn is_target_char(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, '_' | '.' | ':' | '$' | '#' | '@' | '?' | '!')
}

fn should_skip_call(line: &str, target_start: usize, target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    let simple = lower.rsplit(['.', ':']).next().unwrap_or(&lower);
    if CONTROL_CALLEES.contains(&simple) {
        return true;
    }
    let prefix = line[..target_start].trim_end();
    let last_word = prefix.split_whitespace().last().unwrap_or_default();
    if matches!(
        last_word,
        "function"
            | "fn"
            | "func"
            | "def"
            | "void"
            | "int"
            | "string"
            | "bool"
            | "boolean"
            | "public"
            | "private"
            | "protected"
            | "static"
            | "class"
            | "struct"
            | "interface"
    ) {
        return true;
    }
    target.contains("=>")
}

fn matching_close_paren(line: &str, open: usize) -> Option<usize> {
    let mut depth = 0i64;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line[open..].char_indices() {
        let absolute = open + index;
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            quote = Some(character);
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(absolute);
                }
            }
            _ => {}
        }
    }
    None
}

fn summarize_arguments(args: &str, redact_strings: bool) -> Vec<String> {
    split_arguments(args)
        .into_iter()
        .take(8)
        .map(|arg| summarize_argument(&arg, redact_strings))
        .collect()
}

fn split_arguments(args: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i64;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in args.char_indices() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'' | '`') {
            quote = Some(character);
            continue;
        }
        match character {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                values.push(args[start..index].trim().to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    let last = args[start..].trim();
    if !last.is_empty() {
        values.push(last.to_string());
    }
    values
}

fn summarize_argument(arg: &str, redact_strings: bool) -> String {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return "empty".to_string();
    }
    if let Some(first) = string_literals(trimmed).first() {
        return if redact_strings {
            "string:<redacted>".to_string()
        } else {
            format!("string:{}", bounded_value(first))
        };
    }
    if trimmed.parse::<f64>().is_ok() {
        return "number".to_string();
    }
    if matches!(trimmed, "true" | "false") {
        return "boolean".to_string();
    }
    if trimmed.starts_with('{') {
        return "object".to_string();
    }
    if trimmed.starts_with('[') {
        return "array".to_string();
    }
    if trimmed.contains('(') {
        return format!(
            "call:{}",
            trimmed.split('(').next().unwrap_or_default().trim()
        );
    }
    format!("identifier:{}", bounded_value(trimmed))
}

fn split_receiver_callee(target: &str) -> (Option<String>, String) {
    if let Some((receiver, callee)) = target.rsplit_once('.') {
        return (Some(receiver.to_string()), callee.to_string());
    }
    if let Some((receiver, callee)) = target.rsplit_once("::") {
        return (Some(receiver.to_string()), callee.to_string());
    }
    (None, target.to_string())
}

fn is_generic_callee(callee: &str) -> bool {
    let lower = callee.to_ascii_lowercase();
    GENERIC_CALLEES.contains(&lower.as_str())
}

fn callsite_confidence(receiver: Option<&str>, imported: bool, generic: bool) -> f64 {
    let mut confidence: f64 = if generic { 0.35 } else { 0.65 };
    if receiver.is_some() {
        confidence += 0.10;
    }
    if imported {
        confidence += 0.20;
    }
    confidence.min(1.0)
}
