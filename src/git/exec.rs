//! 统一的 git 命令执行器。
//!
//! 所有 git 调用都必须走这里，保证：
//! - 带 `--no-pager`，避免分页器把输出卡住
//! - 带显式 `-C <repo>`，不依赖进程当前目录
//! - 捕获 stdout / stderr，**不吞掉错误**
//! - 有超时，异常时 kill 子进程
//! - 不经 shell，参数直接传给 `git`，避免注入与平台引号差异

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

/// 默认超时。克隆、fetch 等长任务由调用方显式放宽。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// git 调用错误。
#[derive(Debug, Error)]
pub enum GitError {
    /// 无法启动 git（未安装、无权限）。
    #[error("无法启动 git：{0}")]
    Spawn(#[source] std::io::Error),
    /// 执行超时，子进程已被 kill。
    #[error("git 执行超时（{0} 秒）：{1}")]
    Timeout(u64, String),
    /// 命令返回非零。
    #[error("git 命令失败：{command}\n{stderr}")]
    CommandFailed {
        /// 执行过的命令行（已脱敏，不含凭据）。
        command: String,
        /// git 的 stderr 原文。
        stderr: String,
    },
    /// 读取管道失败。
    #[error("读取 git 输出失败：{0}")]
    Io(#[source] std::io::Error),
}

/// 一次 git 调用的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    /// 实际执行的命令行，用于终端回显。
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    /// 退出码是否为 0。
    pub success: bool,
    /// 退出码。
    pub code: Option<i32>,
}

impl GitOutput {
    /// 去掉尾部空白的 stdout。
    pub fn stdout_trimmed(&self) -> &str {
        self.stdout.trim_end()
    }

    /// stdout 按行切分，过滤空行。
    pub fn lines(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect()
    }
}

/// 绑定到某个仓库目录的 git 执行器。
#[derive(Debug, Clone)]
pub struct GitExecutor {
    repo: PathBuf,
    timeout: Duration,
}

impl GitExecutor {
    /// 绑定仓库目录。
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        Self {
            repo: repo.into(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// 调整超时。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 绑定的仓库路径。
    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// 执行命令并返回完整输出（非零退出码也返回，不报错）。
    pub fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
        self.run_with_stdin(args, None)
    }

    /// 执行命令，非零退出码转为 [`GitError::CommandFailed`]。
    pub fn run_ok(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.run(args)?;
        if !out.success {
            return Err(GitError::CommandFailed {
                command: out.command,
                stderr: out.stderr,
            });
        }
        Ok(out.stdout)
    }

    /// 执行命令并把 `stdin_data` 写进标准输入（`git credential approve` 等需要）。
    pub fn run_with_stdin(
        &self,
        args: &[&str],
        stdin_data: Option<&str>,
    ) -> Result<GitOutput, GitError> {
        let command = format!(
            "git --no-pager -C {} {}",
            self.repo.display(),
            args.join(" ")
        );

        let mut cmd = Command::new("git");
        cmd.arg("--no-pager")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .stdin(if stdin_data.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(GitError::Spawn)?;

        if let Some(data) = stdin_data {
            if let Some(mut pipe) = child.stdin.take() {
                let payload = data.to_string();
                // 不 join：git 不读 stdin 时 join 会死锁
                thread::spawn(move || {
                    use std::io::Write;
                    let _ = pipe.write_all(payload.as_bytes());
                });
            }
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError::Io(std::io::Error::other("无法获取 stdout 管道")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| GitError::Io(std::io::Error::other("无法获取 stderr 管道")))?;

        let out_handle = thread::spawn(move || read_all(stdout));
        let err_handle = thread::spawn(move || read_all(stderr));

        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(GitError::Io)? {
                break Some(status);
            }
            if start.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(Duration::from_millis(20));
        };

        let stdout = out_handle.join().unwrap_or_default();
        let stderr = err_handle.join().unwrap_or_default();

        match status {
            None => Err(GitError::Timeout(self.timeout.as_secs(), command)),
            Some(status) => Ok(GitOutput {
                command,
                stdout,
                stderr,
                success: status.success(),
                code: status.code(),
            }),
        }
    }
}

fn read_all(mut pipe: impl Read) -> String {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("临时目录");
        let out = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["init", "-q"])
            .output();
        match out {
            Ok(o) if o.status.success() => dir,
            _ => panic!("无法创建临时 git 仓库，请确认已安装 git"),
        }
    }

    #[test]
    fn runs_inside_real_repo() {
        let dir = temp_repo();
        let ex = GitExecutor::new(dir.path());
        let out = ex
            .run(&["rev-parse", "--is-inside-work-tree"])
            .expect("应能执行");
        assert!(out.success, "stderr: {}", out.stderr);
        assert_eq!(out.stdout_trimmed(), "true");
        assert!(out.command.starts_with("git --no-pager -C "));
    }

    #[test]
    fn non_repo_reports_failure_without_panic() {
        let dir = tempfile::tempdir().expect("临时目录");
        let ex = GitExecutor::new(dir.path());
        let out = ex
            .run(&["rev-parse", "--git-dir"])
            .expect("命令本身应能跑完");
        assert!(!out.success, "非仓库目录应失败");
        assert!(!out.stderr.is_empty(), "应捕获 stderr");
    }

    #[test]
    fn run_ok_turns_failure_into_error() {
        let dir = tempfile::tempdir().expect("临时目录");
        let ex = GitExecutor::new(dir.path());
        let err = ex.run_ok(&["rev-parse", "--git-dir"]).expect_err("应报错");
        match err {
            GitError::CommandFailed { stderr, .. } => assert!(!stderr.is_empty()),
            other => panic!("错误类型不对：{other}"),
        }
    }

    #[test]
    fn timeout_kills_process() {
        let dir = temp_repo();
        let ex = GitExecutor::new(dir.path()).with_timeout(Duration::from_nanos(1));
        let err = ex.run(&["status"]).expect_err("应超时");
        assert!(matches!(err, GitError::Timeout(..)), "实际：{err}");
    }

    #[test]
    fn missing_repo_path_is_reported() {
        let ex = GitExecutor::new("/definitely/not/exists/git-switch-test");
        let out = ex.run(&["status"]).expect("命令应能跑完");
        assert!(!out.success);
    }

    #[test]
    fn lines_are_trimmed() {
        let out = GitOutput {
            command: String::new(),
            stdout: "a\nb\n\n".to_string(),
            stderr: String::new(),
            success: true,
            code: Some(0),
        };
        assert_eq!(out.lines(), vec!["a", "b"]);
        assert_eq!(out.stdout_trimmed(), "a\nb");
    }
}
