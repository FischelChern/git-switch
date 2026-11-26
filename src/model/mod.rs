//! 纯数据与业务规则。**本模块禁止任何 IO。**
//!
//! 三层实体及其关系：
//!
//! ```text
//! Platform  平台，由远程地址的 host 决定
//!   └── Identity  身份：一套 user.name + user.email + 认证方式
//!         └── (被 Repository 引用)  项目：磁盘上的 git 仓库
//! ```
//!
//! 关键不变量：
//! - 平台由 [`remote::RemoteUrl`] 推导，调用方不得凭空指定。
//! - 项目只持有 `identity_id` 引用，**项目不拥有身份，身份不拥有项目**。

pub mod graph;
pub mod highlight;
pub mod identity;
pub mod platform;
pub mod remote;
pub mod repository;
