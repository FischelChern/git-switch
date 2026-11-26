//! git 集成的**唯一**出口。
//!
//! 所有外部命令都必须经过 [`exec::GitExecutor`]，统一带 `--no-pager`、
//! 明确的 `-C <repo>`、超时与 stderr 捕获。
//!
//! 本模块不依赖 `egui`，也不做 UI 决策。

pub mod config;
pub mod exec;

pub use exec::{GitError, GitExecutor, GitOutput};
