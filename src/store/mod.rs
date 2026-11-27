//! 持久化层：配置文件与系统钥匙串。
//!
//! 这一层是**唯一**允许碰文件系统与钥匙串的地方。
//! `model` 不得依赖本模块，`git` 只读取本模块产出的数据。

pub mod authcheck;
pub mod config;
pub mod credential;
pub mod secret;

pub use authcheck::{Check, Level, Report};
pub use config::{Config, config_dir, config_path};
pub use credential::CredStatus;
pub use secret::{delete_secret, get_secret, set_secret};
