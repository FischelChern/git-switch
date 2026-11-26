//! git 集成的**唯一**出口。
//!
//! 所有外部命令都必须经过 [`exec::GitExecutor`]，统一带 `--no-pager`、
//! 明确的 `-C <repo>`、超时与 stderr 捕获。
//!
//! 本模块不依赖 `egui`，也不做 UI 决策。

pub mod branch;
pub mod clone;
pub mod config;
pub mod diff;
pub mod discover;
pub mod exec;
pub mod file;
pub mod log;
pub mod ops;
pub mod status;

pub use branch::{Branch, BranchError};
pub use clone::{CloneError, CloneOutcome, CloneRequest, clone_repo, dir_name_from_repo};
pub use diff::{DiffRow, FileDiff, RowKind, SideRow};
pub use discover::{DiscoveredRepo, ScanOptions};
pub use exec::{GitError, GitExecutor, GitOutput};
pub use file::{
    BlameLine, FileContent, FileError, TreeEntry, blame, find_readme, list_dir, read_file,
    read_readme, write_file,
};
pub use log::{Commit, GraphData, parse_log, read_graph};
pub use ops::{OpError, OpResult};
pub use status::{ChangeStatus, FileChange, WorkStatus, read_status};
