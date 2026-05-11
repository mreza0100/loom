use loom_core::{
    git_analyzer::{parse_git_log, CommandOutput, CommandRunner, GitAnalyzer, SystemCommandRunner},
    LoomConfig, LoomError, Result,
};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn git_parser_filters_and_scores_pairs() {
    let dir = tempdir().unwrap();
    let config = LoomConfig::default_for_target(dir.path());
    let output = "\
---COMMIT---
src/a.py
src/b.py
README.md
---COMMIT---
src/a.py
src/b.py
src/c.ts
---COMMIT---
src/a.py
";

    let pairs = parse_git_log(output, dir.path(), &config.watch_extensions, 10);
    let ab = pairs
        .iter()
        .find(|pair| pair.file_a == "src/a.py" && pair.file_b == "src/b.py")
        .unwrap();
    assert_eq!(ab.frequency, 2);
    assert!(ab.recency > 0.0);
    assert!(!pairs.iter().any(|pair| pair.file_b.ends_with("README.md")));
}

#[derive(Debug)]
struct FakeRunner {
    output: CommandOutput,
}

impl CommandRunner for FakeRunner {
    fn run(&self, _cmd: &mut Command, _timeout: Duration) -> Result<CommandOutput> {
        Ok(self.output.clone())
    }
}

#[test]
fn git_timeout_returns_empty_pairs() {
    let dir = tempdir().unwrap();
    let config = LoomConfig::default_for_target(dir.path());
    let analyzer = GitAnalyzer::new(
        config,
        Arc::new(FakeRunner {
            output: CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
            },
        }),
        Duration::from_secs(1),
    );

    assert!(analyzer.analyze_cochanges().unwrap().is_empty());
}

#[test]
fn git_nonzero_status_propagates_error() {
    let dir = tempdir().unwrap();
    let config = LoomConfig::default_for_target(dir.path());
    let analyzer = GitAnalyzer::new(
        config,
        Arc::new(FakeRunner {
            output: CommandOutput {
                status: 128,
                stdout: String::new(),
                stderr: "fatal".to_string(),
                timed_out: false,
            },
        }),
        Duration::from_secs(1),
    );

    let error = analyzer.analyze_cochanges().unwrap_err();
    assert!(matches!(error, LoomError::GitCommand(message) if message == "fatal"));
}

#[cfg(unix)]
#[test]
fn system_command_runner_enforces_timeout() {
    let runner = SystemCommandRunner;
    let mut command = Command::new("sh");
    command.arg("-c").arg("sleep 2");
    let start = Instant::now();

    let output = runner.run(&mut command, Duration::from_millis(50)).unwrap();

    assert!(output.timed_out);
    assert!(start.elapsed() < Duration::from_secs(1));
}
