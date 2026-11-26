# GitSwitch — 项目约定

> 本文件是本仓库的最高优先级约定。与 `~/.pi/agent/AGENTS.md` 冲突时，以本文件为准。
> 修改本文件属于需求变更，必须先走「demo 先行」流程（见第 4 节）。

---

## 1. 这是什么

一个跨平台的 **多 Git 身份切换器**。解决的问题是：同一台机器上，不同平台（GitHub / 公司 GitLab / Gitee）、不同账号（个人号 / 工作号 / CI 机器人）的仓库混在一起，全局 `user.name` / `user.email` 只有一份，提交署名和推送凭据经常用错。

GitSwitch 管理三层关系，并保证**切换只影响被切换的那个仓库**：

```
平台 (Platform)  ── 决定可用身份集合与远程地址形态
  └─ 身份 (Identity)  ── 一套 git 用户 + 一种认证方式
       └─ 项目 (Repository)  ── 落在磁盘上的 git 仓库，可覆盖身份的用户名/邮箱
```

- 目标平台：**macOS / Linux / Windows**，单二进制，原生 GUI。
- 技术栈：Rust + `eframe` / `egui`。
- 非目标：不做 git 托管平台客户端，不做代码托管，不做 CI。

---

## 2. 核心概念（术语必须一致）

| 术语 | 英文/代码名 | 定义 | 关键点 |
|---|---|---|---|
| 平台 | `Platform` | 一个 git 托管服务的域名，如 `github.com`、`gitlab.company.com` | **由项目的远程地址决定，用户不可手动切换** |
| 身份 | `Identity`（旧称 account） | 一套 `user.name` + `user.email` + 一种认证方式 + 凭据引用 | 属于某个平台；同一平台可有多个身份 |
| 项目 | `Repository` | 磁盘上的一个 git 仓库 | 属于某个平台；**当前使用哪个身份是可切换的** |
| 项目覆盖 | `IdentityOverride` | 项目级的 `user.name` / `user.email` | 优先级最高；未开启时继承身份默认值 |
| 认证方式 | `AuthMode` | `Ssh` / `Token` / `Password` / `CredentialFile` | 决定写 `core.sshCommand` 还是 `credential.helper` |

**身份优先级（从高到低）**：项目覆盖 → 身份默认值 → 不写入（跟随全局）。
**禁止**跨层：项目不能属于身份，身份不能拥有项目。项目只持有 `identity_id` 引用的当前身份。

---

## 3. 已确认需求（不要自行删减）

### 3.1 平台与身份
- 预设平台：GitHub、GitLab、Gitee；支持自定义 host。
- 一个平台可挂多个身份。
- 新增身份支持四种认证：SSH 私钥、访问令牌、账号密码、凭据文件（`.netrc`）。
- 身份的私钥口令 / 令牌 / 密码存入系统钥匙串，配置文件只存**引用**。

### 3.2 项目
- 项目列表按平台展示；中栏显示**该平台下的全部项目**，不随身份切换而变化。
- 每个项目显示当前身份；中栏卡片上有身份徽标。
- 项目可**单独设置** `user.name` / `user.email`（覆盖身份默认值）。
- 支持从平台 API 自动发现「该身份参与的项目」，并可一键克隆。
- 克隆弹窗字段：仓库地址、克隆到、目录名、平台、使用的身份。

### 3.3 git 能力
- 操作：获取、拉取、推送、提交（需 message 输入）、贮藏、分支、合并、变基、重置、标签、日志、PR。
- **提交图（git 树）**：泳道式分支图，可见分支、合并、标签引用；tab 名字直接叫 `git`。
- 文件视图：变更树（M/A/D/R/U）+ 状态色；支持全部文件浏览。
- 编辑器：语法高亮、行内 blame、引用计数（usage）、语法错误标红、`Ctrl/Cmd+Z` 撤销、`Ctrl/Cmd+S` 保存、自动保存、保存时自动格式化。
- Diff：左右对比 / 上下对比两种模式。
- PR：列表、查看、合并、检出。
- 底部终端：命令与输出回显，含「问题」「Git 输出」分页。

### 3.4 界面骨架（三栏 + 底栏）
```
┌──────────────────────────────────────────────┐
│ 顶栏：Logo · 搜索 · [扫描本地仓库]            │
├──────────┬──────────┬────────────────────────┤
│ 平台·身份 │ 项目      │ 详情                   │
│ 树        │ 列表      │ 平台(锁定)/身份(可切)/认证│
│          │          │ git 操作栏              │
│          │          │ tabs: README / git /    │
│          │          │       本地文件 / PR      │
│          │          │ 底部：写入范围提示       │
├──────────┴──────────┴────────────────────────┤
│ 终端 / 问题 / Git 输出                        │
└──────────────────────────────────────────────┘
[设置] [+ 添加平台及身份]        [克隆]
```
- 平台选择器**显示为锁定态**（虚线边框 + 锁图标），点击提示「平台由仓库远程地址决定，不可切换」。
- 身份下拉标签必须写明作用范围：**只作用于当前项目**。

---

## 4. 开发流程：demo 先行（强制）

**任何需求变更、界面调整、交互改动，都必须先产出 HTML 原型（demo），等用户确认效果后才允许写 Rust 实现。**

流程：

1. 在 `demo/` 下新建或修改原型（最新版固定为 `demo/index.html`，历史版本按 `demo/vN.html` 保留）。
2. 用浏览器实际点击验证关键交互，把验证结果写进回复（点了什么、看到什么）。
3. 请用户查看 demo，明确说「确认后再改 Rust」。
4. 用户确认后，才动 `src/` 下的实现。
5. 实现完成后，若界面与 demo 有偏差，必须回写 demo 或在回复中说明差异原因。

**禁止**：跳过 demo 直接改 Rust 界面；demo 改了不同步给用户看；把 demo 当最终交付物。

---

## 5. 硬性约束（不可违反）

### 5.1 不污染用户环境
- **绝不修改 `~/.gitconfig`**，除非用户显式要求并在回复中列出将要写入的确切内容。
- 身份切换只写目标仓库的 `<repo>/.git/config`：
  - `user.name` / `user.email`
  - SSH：`core.sshCommand = ssh -i <key> -o IdentitiesOnly=yes`
  - Token/Password：`credential.helper`（平台相关）+ 把凭据交给系统凭据助手
- 切换 A 项目不得影响 B 项目，必须有测试覆盖。

### 5.2 凭据安全
- 令牌、密码、私钥口令**只进系统钥匙串**：macOS Keychain / Windows Credential Manager / Linux Secret Service。
- 配置文件（JSON）里只允许出现引用标识，禁止出现明文凭据。
- 日志与终端回显禁止打印凭据原文，只允许出现「已保存到钥匙串」之类状态。
- 禁止把凭据写进 `~/.git-credentials`、`.netrc`（除非用户主动选了「凭据文件」方式）。

### 5.3 不伪造
- 任何 git 命令的输出必须来自真实执行结果；禁止在实现里写死成功文案。
- 命令失败必须把 stderr 原样展示，并给出可执行的下一步建议。
- 无法验证的能力（例如未安装的平台 CLI）必须显式说明限制。

### 5.4 平台不可手动切换
- 项目的平台由其 remote URL 推导；若用户修改了 remote 到别的域名，工具必须检测到不一致并提示，不得静默沿用旧平台。

---

## 6. 架构

### 6.1 技术选型

| 层 | 选型 | 理由 |
|---|---|---|
| GUI | `eframe` 0.37 / `egui` 0.37 | 纯 Rust 单二进制，三平台无额外系统依赖 |
| 序列化 | `serde` + `serde_json` | 配置与缓存 |
| 凭据 | `keyring` 4.x | 三平台原生钥匙串 |
| 错误 | `anyhow` | 应用层聚合错误；需要区分类型时用自定义枚举 + `thiserror` |
| 路径 | `dirs` | `config_dir()` 定位配置目录 |
| 进程 | `std::process::Command`（**不经 shell**） | 避免注入与平台差异 |

暂不引入：Tauri（webview 跨平台依赖重）、clap（参数极少）、噪音重的 ORM/DB（配置用 JSON 足够）。

### 6.2 模块划分（单向依赖，禁止反向）

```
src/
├── main.rs            程序入口：解析极少参数、初始化日志、启动 eframe
├── app.rs             App 状态机：tab 切换、选中项、事件分发（不含业务规则）
├── model/             ★ 纯数据与规则，零 IO、可单测
│   ├── mod.rs
│   ├── platform.rs    Platform、PlatformKind、host 匹配
│   ├── identity.rs    Identity、AuthMode、凭据引用
│   ├── repository.rs  Repository、IdentityOverride、有效身份解析
│   └── remote.rs      RemoteUrl 解析（ssh scp / https / ssh://）、平台推导
├── store/             持久化
│   ├── mod.rs
│   ├── config.rs      Config 读写（原子写：临时文件 + rename）
│   ├── secret.rs      钥匙串封装（set/get/delete）
│   └── prefs.rs       界面偏好（自动保存、blame 开关等）
├── git/               ★ 唯一允许执行 git 的地方
│   ├── mod.rs
│   ├── exec.rs        统一命令执行器：捕获 stdout/stderr、超时、错误类型
│   ├── config.rs      local config 读写、override 应用、回滚
│   ├── status.rs      状态、变更列表、分支、ahead/behind
│   ├── log.rs         git 树（提交 + parents + refs）与泳道布局输入
│   ├── diff.rs        unified diff 解析、左右/上下渲染数据
│   └── remote_api.rs  平台 API：列出身份参与的项目（GitHub/GitLab/Gitee）
└── ui/                ★ 只渲染与转发事件
    ├── mod.rs
    ├── theme.rs       颜色、字号、间距常量
    ├── fonts.rs       CJK 字体加载（三平台候选路径）
    ├── layout.rs      三栏 + 终端布局
    ├── widgets/       通用控件：下拉、树行、状态徽标、开关
    └── panels/
        ├── sidebar.rs     平台·身份树
        ├── projects.rs    项目列表
        ├── detail.rs      详情容器
        ├── git_tree.rs    提交图（泳道）
        ├── files.rs       变更/全部文件 + diff
        ├── editor.rs      代码编辑器（高亮层 + 透明 textarea 方案）
        ├── pull_requests.rs
        ├── terminal.rs
        └── modals/        克隆、新增身份、设置、提交信息
```

**依赖方向**：`ui → git → model ← store`。`model` 不得依赖任何其它模块；`ui` 不得直接调 `git::exec`；`git` 不得碰 `egui` 类型。

### 6.3 数据流

```
用户操作 → ui 发出 Action
            ↓
        app.rs 处理（改内存状态 / 调 git / 调 store）
            ↓
        git::* 执行真实命令 → 得到结果或错误
            ↓
        结果回写 model → 触发 ui 重绘
            ↓
        终端面板追加命令与输出
```

长任务（克隆、fetch、列表 API）走后台线程 + `mpsc` 回传进度，**UI 线程不得阻塞**。

### 6.4 配置文件

- 路径：`<config_dir>/git-switch/config.json`（macOS: `~/Library/Application Support/git-switch/`）
- 覆盖：环境变量 `GIT_SWITCH_CONFIG_DIR`（测试与便携模式）
- 写入方式：**原子写**（写 `.tmp` 后 rename），禁止直接截断写
- 结构：

```json
{
  "version": 1,
  "platforms": [
    { "id": "github", "name": "GitHub", "host": "github.com", "kind": "github" }
  ],
  "identities": [
    { "id": "gh-me", "platform_id": "github", "label": "张鑫宇 · 个人",
      "user_name": "张鑫宇", "user_email": "429092941@qq.com",
      "auth": "ssh", "ssh_key_path": "~/.ssh/id_ed25519_github",
      "secret_ref": "git-switch:identity:gh-me" }
  ],
  "repositories": [
    { "id": "r1", "platform_id": "github", "identity_id": "gh-me",
      "path": "/Users/mac/WorkSpace/iot-gateway",
      "remote_url": "git@github.com:zhangxinyu/iot-gateway.git",
      "override": { "enabled": false, "user_name": null, "user_email": null } }
  ]
}
```

---

## 7. 目录结构

```
git_switch/
├── AGENTS.md              本文件
├── README.md              用户向说明（安装、使用、截图）
├── Cargo.toml
├── Cargo.lock
├── demo/                  ★ HTML 原型（需求变更先在这里改）
│   ├── index.html         最新版，固定入口
│   ├── vN.html            历史版本
│   └── README.md          原型说明与版本记录
├── docs/
│   ├── requirements.md    需求条目与验收标准（AT）
│   └── backlog.md         缺陷与任务清单（与第 9 节同步）
├── src/                   见 6.2
└── tests/
    ├── model_remote.rs    远程地址解析与平台推导
    ├── model_identity.rs  身份优先级（覆盖 > 身份）
    └── git_config.rs      真实临时仓库：写入隔离性、回滚
```

---

## 8. 跨平台规则

| 关注点 | macOS | Linux | Windows |
|---|---|---|---|
| 配置目录 | `~/Library/Application Support` | `~/.config` | `%APPDATA%` |
| 凭据助手 | `osxkeychain` | `libsecret`（无桌面则报错并提示） | `manager` |
| 路径分隔符 | `/` | `/` | 内部统一 `PathBuf`，**禁止字符串拼接路径** |
| `~` 展开 | 自行展开为 home，不得依赖 shell | 同 | 同（`%USERPROFILE%`） |
| 字体 | `/System/Library/Fonts/Supplemental/Arial Unicode.ttf` 等 | Noto CJK 系列路径 | `C:\Windows\Fonts\msyh.ttc` |
| 命令执行 | `Command::new("git")`，**不经 shell** | 同 | 同（避免 `cmd /c` 引号地狱） |

- `ssh -i` 的路径若含空格，需要按平台正确加引号，写进 `core.sshCommand` 时整体作为一个值。
- 所有平台差异集中在 `store/` 与 `git/exec.rs`，`model/` 与 `ui/` 不得出现 `cfg!(target_os)` 分支。

---

## 9. 已知缺陷与 backlog

> 来源：对照 demo v5 的逐项核查。**P0 未清空前不进入新特性开发。**
> 任务化清单与验收标准见 `docs/backlog.md`，两处必须同步。


### P0 — 回归遗漏（用户明确要求过，demo 中已丢失）
- [ ] **仓库级 `user.name` / `user.email` 覆盖**：`Repository` 无 `override` 字段，界面无入口。需补数据模型 + 详情面板「项目覆盖」开关与输入框 + 写入/清除逻辑 + 优先级测试。
- [ ] **认证方式改的是身份而非项目**：`authSeg` 修改 `identity.auth`，会影响所有使用该身份的项目。需明确语义：项目级覆盖 `AuthMode`，或改为只读展示并标注影响范围。
- [ ] **凭据状态不可见**：新增身份后无法确认令牌是否已入钥匙串，缺「已保存 / 未保存」与「测试连接」。

### P1 — 有壳无实现
- [ ] 搜索框无事件绑定（demo 中为装饰）
- [ ] 「扫描本地仓库」为假动作
- [ ] 提交无 message 输入弹窗
- [ ] Diff 仅一个样例文件
- [x] README 读本地文件（`git/file.rs` + `find_readme` / `read_readme`）
- [ ] 本地文件缺「全部文件」浏览（仅变更）
- [x] 克隆「克隆到」无目录选择器（已加目标路径实时预览与浏览入口）
- [ ] 分支仅 checkout，不能新建/删除/重命名
- [ ] 标签仅列出
- [ ] 项目 / 身份 / 平台均无删除与编辑入口

### P2 — 真实使用缺口
- [ ] 合并冲突无处理界面
- [ ] git 命令失败无统一错误呈现
- [ ] 无 reflog / 撤销入口
- [ ] 项目平台与 remote 域名不一致时无检测提示
- [ ] 同 host 多平台时 `platByHost` 歧义
- [ ] 无右键菜单（复制路径、Finder/终端打开）
- [ ] 无首次启动引导（导入现有身份、选扫描目录）

### P3 — 工程化
- [ ] Rust 侧零实现（仅空 `main.rs`）
- [ ] 无单元 / 集成测试
- [ ] 打包、签名、自动更新未规划

---

## 10. 编码规范

- Rust 2024 edition；`cargo fmt` 与 `cargo clippy -D warnings` 必须通过。
- 公开 API 写文档注释；模块顶部写职责与不变量。
- 错误：库内用 `thiserror` 定义具体错误；应用层用 `anyhow` 加 `context()`（例如 `.context("写入 .git/config 失败")`）。
- **禁止** `unwrap()` / `expect()` 出现在非测试代码；用 `?` 或显式 `ok_or_else`。
- 禁止 `panic!` 处理预期内的错误（找不到 keyring、git 返回非零等）。
- 所有外部命令必须经 `git/exec.rs`，统一带 `--no-pager`、明确的 `-C <repo>`、超时与 stderr 捕获。
- 路径一律 `PathBuf` / `&Path`；字符串路径只在展示时出现。
- UI 文案统一简体中文；技术术语（SSH、Token、`.git/config`）保留原文。
- 命名遵循 demo 中的中文标签，避免「账号/身份」混用——代码里统一 `identity`。

---

## 11. 测试策略

| 层 | 方式 | 要求 |
|---|---|---|
| `model/` | 纯单测 | 远程地址解析、平台匹配、身份优先级、覆盖生效/失效 |
| `git/config` | 集成测试 | 用 `tempfile` 建真实 git 仓库，验证只写目标仓库、A 不影响 B、失败可回滚 |
| `git/log` | 集成测试 | 构造含合并/分支的仓库，验证泳道布局输入正确 |
| `git/remote_api` | 单测 + mock | 只测响应解析，不打真实网络 |
| `ui/` | 手动 | 以 demo 为视觉基准，逐屏比对 |

- `cargo test` 必须在三平台 CI 上跑通；Windows 相关逻辑用 `cfg` 分支覆盖。
- 任何涉及文件写入的测试必须使用临时目录，禁止触碰用户真实配置与仓库。

---

## 12. 提交规范

- 提交信息用中文，格式：`<type>(<scope>): <说明>`，type ∈ `feat|fix|refactor|docs|test|chore`。
- 例：`feat(identity): 支持项目级 user.email 覆盖`
- 一次提交只做一件事；禁止把 demo 改动与 Rust 实现混在同一次提交。
- **禁止** `git push --force`、`git reset --hard` 到共享分支。

---

## 13. 禁止事项

1. 跳过 demo 直接改 Rust 界面。
2. 修改 `~/.gitconfig`。
3. 在配置文件、日志、终端回显中出现明文凭据。
4. 用字符串拼接路径或手写 shell 命令字符串执行。
5. 在 `model/` 里做 IO，在 `ui/` 里执行 git 命令。
6. 伪造命令输出、测试结果或「已修复」的结论。
7. 删除或改写用户已有的未提交变更。
