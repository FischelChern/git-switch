//! GitSwitch 库入口。
//!
//! 模块依赖方向（单向，禁止反向）：
//!
//! ```text
//! ui → git → model ← store
//! ```
//!
//! - [`model`]：纯数据与业务规则，**零 IO**，可单元测试。
//! - [`store`]：持久化（配置文件、系统钥匙串）。
//! - [`git`]：**唯一**允许执行 git 命令的地方。
//! - `ui`：只负责渲染与事件转发（尚未实现）。
pub mod git;
pub mod model;
pub mod store;
