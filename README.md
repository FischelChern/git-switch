# GitSwitch

跨平台（macOS / Linux / Windows）的多 Git 身份切换器。按「平台 → 身份 → 项目」三层管理 `user.name`/`user.email` 与推送凭据，**切换只影响被切换的那一个仓库**。

## 状态

**当前阶段：Rust 骨架可运行。**

已完成：数据模型、配置持久化、钥匙串封装、git 本地配置读写（含回滚）、三栏界面与项目级覆盖。
未完成：git 树、文件浏览、编辑器、diff、PR、克隆与扫描（见 `docs/backlog.md`）。

- HTML 原型：[`demo/index.html`](demo/index.html)（浏览器直接打开即可）
- 项目约定与架构：[`AGENTS.md`](AGENTS.md)
- 需求与验收标准：[`docs/requirements.md`](docs/requirements.md)
- 缺陷与任务：[`docs/backlog.md`](docs/backlog.md)

## 为什么需要它

同一台机器上混着个人 GitHub、公司 GitLab、Gitee 的仓库。全局 `user.name` 只有一份，结果：

- 提交记录里出现错误的署名邮箱
- `push` 时用错 SSH 密钥或令牌，报 `Permission denied`
- 想给某个仓库单独设邮箱，只能手工 `git config --local`，还容易漏

## 它做什么

| 层级 | 内容 |
|---|---|
| 平台 | GitHub / GitLab / Gitee / 自定义域名，由仓库 remote 自动识别，**不可手动切换** |
| 身份 | 一套用户名 + 邮箱 + 认证方式（SSH 密钥 / 令牌 / 账号密码 / 凭据文件） |
| 项目 | 磁盘上的仓库，可选择用哪个身份；也可单独覆盖用户名/邮箱 |

切换身份时只写该仓库的 `.git/config`，**不修改 `~/.gitconfig`**。令牌与密码存入系统钥匙串。

## 主要界面

三栏布局：左侧「平台 · 身份」树、中间该项目所属平台下的全部项目、右侧详情（平台锁定 / 身份可切 / git 操作 / README / git 树 / 本地文件 / PR），底部是终端。

原型里可以看到完整的提交图（泳道 + 分支 + 合并 + 标签）、变更文件树、代码编辑器（语法高亮 / 行内 blame / 引用计数 / 语法错误标红 / 自动保存）以及左右与上下两种 diff 视图。

## 开发流程

本项目采用 **demo 先行**：

1. 需求或界面变更，先改 `demo/` 下的 HTML 原型
2. 用浏览器实际点击验证，把结果写进回复
3. 用户确认效果后，才写 Rust 实现

详见 [`AGENTS.md`](AGENTS.md) 第 4 节。

## 构建（待 Rust 实现落地）

```bash
cargo run                 # 启动界面（首次运行使用内置预设平台）
cargo test                # 单元 + 集成测试
cargo build --release     # 产出单二进制
```

三平台交叉编译与打包尚未配置。
