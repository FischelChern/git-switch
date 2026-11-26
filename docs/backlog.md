# 任务清单

与 `AGENTS.md` 第 9 节同步维护。**P0 未清空前不进入新特性开发。**
状态：`todo` / `doing` / `done` / `blocked`

## P0 — 回归遗漏（用户明确要求过，原型中已丢失）

| ID | 任务 | 验收标准 | 状态 |
|---|---|---|---|
| P0-1 | 项目级 `user.name` / `user.email` 覆盖：数据模型加 `IdentityOverride`，详情面板加开关与输入框，写入/清除 `.git/config` | 见 requirements R2.3 的 4 条 AT | todo |
| P0-2 | 修正「认证」语义：现在是改 `identity.auth` 会影响所有使用该身份的项目 | 要么改为项目级覆盖 `AuthMode`，要么只读展示并明示影响范围（N 个项目） | todo |
| P0-3 | 凭据状态可见 + 测试连接 | 新增身份后显示「已保存/未保存」，可点「测试连接」得到真实结果 | todo |

## P1 — 有壳无实现

| ID | 任务 | 状态 |
|---|---|---|
| P1-1 | 搜索框接事件，按平台/身份/项目过滤 | todo |
| P1-2 | 「扫描本地仓库」真实扫描指定目录 | todo |
| P1-3 | 提交弹窗（message 输入，空值拦截） | todo |
| P1-4 | Diff 走真实 `git diff`，去掉固定样例 | todo |
| P1-5 | README 读本地 `README.md` | todo |
| P1-6 | 文件视图加「全部文件 / 仅变更」切换 | todo |
| P1-7 | 克隆「克隆到」加目录选择器 | todo |
| P1-8 | 分支新建 / 删除 / 重命名 | todo |
| P1-9 | 标签创建 / 删除 | todo |
| P1-10 | 项目 / 身份 / 平台 的删除与编辑入口 | todo |

## P2 — 真实使用缺口

| ID | 任务 | 状态 |
|---|---|---|
| P2-1 | 合并冲突检测与解决界面 | todo |
| P2-2 | git 命令失败的统一错误呈现（stderr + 建议） | todo |
| P2-3 | reflog 与「撤销上一次操作」 | todo |
| P2-4 | 项目平台与 remote 域名不一致的检测提示 | todo |
| P2-5 | 同 host 多平台的歧义消解 | todo |
| P2-6 | 右键菜单（复制路径、在 Finder/终端打开） | todo |
| P2-7 | 首次启动引导（导入现有身份、选扫描目录） | todo |

## P3 — 工程化

| ID | 任务 | 状态 |
|---|---|---|
| P3-1 | Rust 骨架：`model` 四个模块 + 单测 | todo |
| P3-2 | `store/config.rs` 原子写 + `store/secret.rs` 钥匙串 | todo |
| P3-3 | `git/exec.rs` 统一执行器 | todo |
| P3-4 | `git/config.rs` 写入与回滚 + 隔离性集成测试 | todo |
| P3-5 | `ui` 三栏骨架对齐 demo | todo |
| P3-6 | 三平台构建与打包、CI | todo |

---

## 已完成

| 日期 | 内容 |
|---|---|
| — | demo v1–v5 迭代：三栏布局、git 树、身份模型修正、克隆弹窗 |
| — | 建立项目骨架与 `AGENTS.md` |
