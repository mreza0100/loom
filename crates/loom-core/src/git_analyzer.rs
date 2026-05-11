use crate::{error::LoomError, indexer::path, LoomConfig, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const COMMIT_SENTINEL: &str = "---COMMIT---";

#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, cmd: &mut Command, timeout: Duration) -> Result<CommandOutput>;
}

#[derive(Debug, Clone)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, cmd: &mut Command, timeout: Duration) -> Result<CommandOutput> {
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| LoomError::GitCommand(source.to_string()))?;
        let deadline = Instant::now() + timeout;
        loop {
            if child
                .try_wait()
                .map_err(|source| LoomError::GitCommand(source.to_string()))?
                .is_some()
            {
                let output = child
                    .wait_with_output()
                    .map_err(|source| LoomError::GitCommand(source.to_string()))?;
                return Ok(CommandOutput {
                    status: output.status.code().unwrap_or(1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    timed_out: false,
                });
            }
            if Instant::now() >= deadline {
                child
                    .kill()
                    .map_err(|source| LoomError::GitCommand(source.to_string()))?;
                let output = child
                    .wait_with_output()
                    .map_err(|source| LoomError::GitCommand(source.to_string()))?;
                return Ok(CommandOutput {
                    status: output.status.code().unwrap_or(1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CochangePair {
    pub file_a: String,
    pub file_b: String,
    pub frequency: i64,
    pub recency: f64,
}

pub struct GitAnalyzer<R: CommandRunner> {
    config: LoomConfig,
    runner: Arc<R>,
    timeout: Duration,
}

impl<R: CommandRunner> GitAnalyzer<R> {
    pub fn new(config: LoomConfig, runner: Arc<R>, timeout: Duration) -> Self {
        Self {
            config,
            runner,
            timeout,
        }
    }

    pub fn is_git_repo(&self) -> Result<bool> {
        let mut cmd = Command::new("git");
        cmd.arg("rev-parse")
            .arg("--is-inside-work-tree")
            .current_dir(&self.config.target_dir);
        let output = self.runner.run(&mut cmd, self.timeout)?;
        Ok(!output.timed_out && output.status == 0)
    }

    pub fn analyze_cochanges(&self) -> Result<Vec<CochangePair>> {
        let mut cmd = Command::new("git");
        cmd.arg("log")
            .arg("--follow")
            .arg("--name-only")
            .arg(format!("--max-count={}", self.config.git_max_commits))
            .arg(format!("--pretty=format:{COMMIT_SENTINEL}"))
            .current_dir(&self.config.target_dir);
        let output = self.runner.run(&mut cmd, self.timeout)?;
        if output.timed_out {
            tracing::warn!("git log timed out; skipping evolutionary coupling");
            return Ok(Vec::new());
        }
        if output.status != 0 {
            return Err(LoomError::GitCommand(output.stderr));
        }
        Ok(parse_git_log(
            &output.stdout,
            &self.config.target_dir,
            &self.config.watch_extensions,
            self.config.git_max_files_per_commit,
        ))
    }
}

pub fn parse_git_log(
    stdout: &str,
    target_dir: &Path,
    watch_extensions: &BTreeSet<String>,
    max_files_per_commit: usize,
) -> Vec<CochangePair> {
    let mut frequency = BTreeMap::<(String, String), i64>::new();
    let mut recency = BTreeMap::<(String, String), f64>::new();

    for (commit_index, block) in stdout.split(COMMIT_SENTINEL).enumerate() {
        let mut files = block
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| {
                Path::new(line)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .map(|extension| watch_extensions.contains(&format!(".{extension}")))
                    .unwrap_or(false)
            })
            .map(|line| normalize_git_path(line, target_dir))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if files.len() < 2 || files.len() > max_files_per_commit {
            continue;
        }
        files.sort();
        for left in 0..files.len() {
            for right in (left + 1)..files.len() {
                let key = (files[left].clone(), files[right].clone());
                *frequency.entry(key.clone()).or_insert(0) += 1;
                let increment = 1.0 / (1.0 + commit_index as f64);
                *recency.entry(key).or_insert(0.0) += increment;
            }
        }
    }

    frequency
        .into_iter()
        .map(|((file_a, file_b), count)| CochangePair {
            recency: recency
                .get(&(file_a.clone(), file_b.clone()))
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0),
            file_a,
            file_b,
            frequency: count,
        })
        .collect()
}

fn normalize_git_path(line: &str, target_dir: &Path) -> String {
    let path = Path::new(line);
    if path.is_absolute() {
        path.strip_prefix(target_dir)
            .map(path::normalize_path)
            .unwrap_or_else(|_| path::normalize_path(path))
    } else {
        path::normalize_path(path)
    }
}
