//! 应用状态机：持有配置、当前选择、终端日志与提示。
//!
//! 这里**不做渲染**（渲染在 `crate::ui`），也**不直接拼命令**
//! （写盘统一走 `git_switch::git::config`）。

use std::collections::HashMap;

use eframe::egui;

use git_switch::git::clone::{CloneError, CloneRequest};
use git_switch::git::config as gitcfg;
use git_switch::git::file as gitfile;
use git_switch::git::{
    self as gitmod, Branch, FileDiff, GraphData, WorkStatus, branch as gitbranch, clone,
    diff as gitdiff, discover, ops,
};
use git_switch::model::highlight::{self, Diagnostic, Language, Token};
use git_switch::model::identity::Identity;
use git_switch::model::remote::RemoteUrl;
use git_switch::model::repository::{IdentityOverride, Repository};
use git_switch::store::Config;

use crate::ui;

/// 日志行的类型，决定颜色。
/// 详情区的 tab。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    /// 概览：平台 / 身份 / 项目覆盖。
    Overview,
    /// 提交图。
    Git,
    /// 工作区变更。
    Files,
    /// 差异视图。
    Diff,
    /// README（读本地文件）。
    Readme,
    /// 代码编辑器。
    Editor,
}

impl DetailTab {
    /// tab 标题。
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "概览",
            Self::Git => "git",
            Self::Files => "文件",
            Self::Diff => "diff",
            Self::Readme => "README",
            Self::Editor => "编辑器",
        }
    }

    /// 全部 tab，按显示顺序。
    pub fn all() -> [Self; 6] {
        [
            Self::Overview,
            Self::Git,
            Self::Files,
            Self::Diff,
            Self::Readme,
            Self::Editor,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    /// 执行的命令。
    Cmd,
    /// 普通输出。
    Out,
    /// 成功。
    Ok,
    /// 错误。
    Err,
}

/// 一行终端日志。
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub kind: LogKind,
    pub text: String,
}

impl LogEntry {
    fn new(kind: LogKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

/// 项目覆盖的编辑缓冲：UI 输入先落在这里，点「应用」才写进配置与磁盘。
#[derive(Debug, Clone, Default)]
pub struct OverrideBuf {
    pub on: bool,
    pub name: String,
    pub email: String,
}

/// 编辑器里打开的文档。
///
/// `saved_text` 是磁盘上的内容，`text` 是编辑缓冲；两者不等即「未保存」。
#[derive(Debug, Clone)]
pub struct OpenDoc {
    /// 相对仓库根的路径。
    pub path: String,
    /// 编辑缓冲。
    pub text: String,
    /// 磁盘上的内容，用于判断脏标记与放弃修改。
    pub saved_text: String,
    /// 识别到的语言。
    pub lang: Language,
    /// 语法错误诊断。
    pub diags: Vec<Diagnostic>,
    /// 每行的着色 token（按行缓存）。
    pub tokens: Vec<Vec<Token>>,
    /// 每行的 blame 信息；未跟踪文件或读取失败时为空。
    pub blame: Vec<gitfile::BlameLine>,
    /// blame 读取失败的原因（不阻断编辑）。
    pub blame_error: Option<String>,
}

impl OpenDoc {
    /// 是否有未保存的修改。
    pub fn dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// 重新计算着色与诊断。
    pub fn reanalyze(&mut self) {
        self.tokens = highlight::highlight(&self.text, self.lang);
        self.diags = highlight::diagnose(&self.text, self.lang);
    }
}

/// 克隆弹窗的状态。
#[derive(Debug, Clone, Default)]
pub struct CloneBuf {
    /// 是否显示弹窗。
    pub open: bool,
    /// 仓库地址。
    pub url: String,
    /// 克隆到的父目录。
    pub parent: String,
    /// 目录名，留空则按远程仓库名推导。
    pub dir_name: String,
    /// 平台 id（由地址推导，展示用）。
    pub platform_hint: String,
    /// 使用的身份 id。
    pub identity_id: String,
    /// 上次操作的错误信息。
    pub error: Option<String>,
    /// 生成中（避免重复点击）。
    pub busy: bool,
}

/// 整个应用的状态。
pub struct App {
    /// 从磁盘加载的配置。
    pub cfg: Config,
    /// 当前选中的平台 id。
    pub platform: Option<String>,
    /// 当前选中的项目 id。
    pub repo: Option<String>,
    /// 项目覆盖的编辑缓冲，按项目 id 索引。
    pub overrides: HashMap<String, OverrideBuf>,
    /// 展开的平台（左侧树的折叠状态）。
    pub expanded: Vec<String>,
    /// 终端日志。
    pub log: Vec<LogEntry>,
    /// 顶部提示：(文字, 剩余秒数)。
    pub toast: Option<(String, f32)>,
    /// 配置文件路径，展示用。
    pub config_path: String,
    /// 实际使用的中文字体路径，`None` 表示没找到。
    pub font_path: Option<String>,
    /// 详情区当前 tab。
    pub tab: DetailTab,
    /// 当前项目的提交图，读取失败时为 `None`。
    pub graph: Option<GraphData>,
    /// 提交图读取失败的原因。
    pub graph_error: Option<String>,
    /// 当前项目的工作区状态。
    pub status: Option<WorkStatus>,
    /// 当前项目的 diff。
    pub diffs: Vec<FileDiff>,
    /// diff 读取失败的原因。
    pub diff_error: Option<String>,
    /// diff 视图模式：true 为左右并排。
    pub diff_side_by_side: bool,
    /// 当前分支列表。
    pub branches: Vec<Branch>,
    /// 标签列表。
    pub tags: Vec<String>,
    /// 分支输入框内容。
    pub branch_input: String,
    /// 提交信息输入框内容。
    pub commit_message: String,
    /// 扫描目录输入框内容。
    pub scan_dir: String,
    /// 上次扫描结果。
    pub scanned: Vec<String>,
    /// 编辑器里打开的文档。
    pub doc: Option<OpenDoc>,
    /// README 内容缓存：(相对路径, 行列表)。
    pub readme: Option<(String, Vec<String>)>,
    /// README 读取失败的原因。
    pub readme_error: Option<String>,
    /// 克隆弹窗状态。
    pub clone_buf: CloneBuf,
    /// 编辑器是否显示 blame 列。
    pub show_blame: bool,
}

impl App {
    /// 构造：加载主题、字体与配置。
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        ui::theme::apply(&cc.egui_ctx);
        let font_path = ui::fonts::install(&cc.egui_ctx);

        let mut log = Vec::new();
        let cfg_path = match git_switch::store::config_path() {
            Ok(p) => p.display().to_string(),
            Err(e) => {
                log.push(LogEntry::new(LogKind::Err, format!("配置路径不可用：{e}")));
                String::from("(未知)")
            }
        };

        let cfg = match Config::load() {
            Ok(c) => {
                log.push(LogEntry::new(
                    LogKind::Ok,
                    format!(
                        "已加载配置：{} 平台 / {} 身份 / {} 项目",
                        c.platforms.len(),
                        c.identities.len(),
                        c.repositories.len()
                    ),
                ));
                c
            }
            Err(e) => {
                log.push(LogEntry::new(
                    LogKind::Err,
                    format!("读取配置失败，使用默认配置：{e}"),
                ));
                Config::default()
            }
        };

        if font_path.is_none() {
            log.push(LogEntry::new(
                LogKind::Err,
                "未找到中文字体，界面中文可能显示为方块",
            ));
        }

        let platform = cfg.platforms.first().map(|p| p.id.clone());
        let repo = platform
            .as_deref()
            .and_then(|pid| cfg.repositories_of(pid).first().map(|r| r.id.clone()));

        let expanded = cfg.platforms.iter().map(|p| p.id.clone()).collect();

        let mut app = Self {
            cfg,
            platform,
            repo,
            overrides: HashMap::new(),
            expanded,
            log,
            toast: None,
            config_path: cfg_path,
            font_path,
            tab: DetailTab::Overview,
            graph: None,
            graph_error: None,
            status: None,
            diffs: Vec::new(),
            diff_error: None,
            diff_side_by_side: true,
            branches: Vec::new(),
            tags: Vec::new(),
            branch_input: String::new(),
            commit_message: String::new(),
            scan_dir: default_scan_dir(),
            scanned: Vec::new(),
            doc: None,
            readme: None,
            readme_error: None,
            clone_buf: CloneBuf::default(),
            show_blame: true,
        };
        app.reload_repo();
        app
    }

    // ---- 查询 ----

    /// 当前平台。
    pub fn current_platform(&self) -> Option<&git_switch::model::platform::Platform> {
        self.platform
            .as_deref()
            .and_then(|id| self.cfg.platform(id))
    }

    /// 当前项目。
    pub fn current_repo(&self) -> Option<&Repository> {
        self.repo.as_deref().and_then(|id| self.cfg.repository(id))
    }

    /// 当前平台下的全部项目（与身份无关）。
    pub fn repos_of_current_platform(&self) -> Vec<Repository> {
        match self.platform.as_deref() {
            Some(pid) => self
                .cfg
                .repositories
                .iter()
                .filter(|r| r.platform_id == pid)
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    }

    /// 某平台下的身份。
    pub fn identities_of(&self, platform_id: &str) -> Vec<Identity> {
        self.cfg
            .identities
            .iter()
            .filter(|i| i.platform_id == platform_id)
            .cloned()
            .collect()
    }

    /// 某项目当前使用的身份。
    pub fn identity_of(&self, repo: &Repository) -> Option<&Identity> {
        repo.identity_id
            .as_deref()
            .and_then(|id| self.cfg.identity(id))
    }

    /// 取（或初始化）项目覆盖缓冲。
    pub fn override_buf(&mut self, repo_id: &str) -> &mut OverrideBuf {
        if !self.overrides.contains_key(repo_id) {
            let (on, name, email) = self
                .cfg
                .repository(repo_id)
                .map(|r| {
                    let o = &r.identity_override;
                    (
                        o.enabled,
                        o.user_name.clone().unwrap_or_default(),
                        o.user_email.clone().unwrap_or_default(),
                    )
                })
                .unwrap_or((false, String::new(), String::new()));
            self.overrides
                .insert(repo_id.to_string(), OverrideBuf { on, name, email });
        }
        self.overrides
            .get_mut(repo_id)
            .expect("刚插入的 key 必然存在")
    }

    // ---- 动作 ----

    /// 选中平台，并自动选中该平台第一个项目。
    pub fn select_platform(&mut self, platform_id: &str) {
        self.platform = Some(platform_id.to_string());
        self.repo = self
            .cfg
            .repositories_of(platform_id)
            .first()
            .map(|r| r.id.clone());
    }

    /// 选中项目。
    pub fn select_repo(&mut self, repo_id: &str) {
        self.repo = Some(repo_id.to_string());
        if let Some(pid) = self.cfg.repository(repo_id).map(|r| r.platform_id.clone()) {
            self.platform = Some(pid);
        }
        self.reload_repo();
    }

    /// 重新读取当前项目的提交图与工作区状态。
    pub fn reload_repo(&mut self) {
        self.graph = None;
        self.graph_error = None;
        self.status = None;

        let Some(repo) = self.current_repo().cloned() else {
            return;
        };
        let path = repo.path.clone();

        self.push_log(
            LogKind::Cmd,
            format!("git -C {} log --all -n200", path.display()),
        );
        match gitmod::read_graph(&path, 200) {
            Ok(g) => {
                self.push_log(
                    LogKind::Out,
                    format!(
                        "{} 条提交 · {} 条泳道",
                        g.commits.len(),
                        g.layout.lane_count
                    ),
                );
                self.graph = Some(g);
            }
            Err(e) => {
                self.graph_error = Some(e.to_string());
                self.push_log(LogKind::Err, format!("读取提交历史失败：{e}"));
            }
        }

        match gitmod::read_status(&path) {
            Ok(s) => {
                self.push_log(
                    LogKind::Out,
                    format!(
                        "分支 {} · {} 个变更",
                        s.branch.clone().unwrap_or_else(|| "（分离头指针）".into()),
                        s.files.len()
                    ),
                );
                self.status = Some(s);
            }
            Err(e) => self.push_log(LogKind::Err, format!("读取工作区状态失败：{e}")),
        }

        self.diffs.clear();
        self.diff_error = None;
        match gitdiff::read_worktree_diff(&path) {
            Ok(d) => {
                if !d.is_empty() {
                    self.push_log(LogKind::Out, format!("{} 个文件有差异", d.len()));
                }
                self.diffs = d;
            }
            Err(e) => {
                self.diff_error = Some(e.to_string());
                self.push_log(LogKind::Err, format!("读取 diff 失败：{e}"));
            }
        }

        match gitbranch::list_branches(&path) {
            Ok(b) => self.branches = b,
            Err(e) => self.push_log(LogKind::Err, format!("读取分支失败：{e}")),
        }
        match gitbranch::list_tags(&path) {
            Ok(t) => self.tags = t,
            Err(e) => self.push_log(LogKind::Err, format!("读取标签失败：{e}")),
        }
    }

    // ---- git 写操作 ----

    /// 当前项目路径；没有选中项目时返回 `None`。
    fn repo_path(&self) -> Option<std::path::PathBuf> {
        self.current_repo().map(|r| r.path.clone())
    }

    /// 记一条命令并返回路径，供写操作复用。
    fn begin_op(&mut self, command: &str) -> Option<std::path::PathBuf> {
        let path = self.repo_path()?;
        self.push_log(LogKind::Cmd, command.to_string());
        Some(path)
    }

    fn finish_op(&mut self, result: Result<ops::OpResult, ops::OpError>, on_ok: &str) {
        match result {
            Ok(r) => {
                for line in r.display().lines() {
                    self.push_log(LogKind::Out, line.to_string());
                }
                self.flash(on_ok.to_string());
            }
            Err(e) => self.push_log(LogKind::Err, e.to_string()),
        }
        self.reload_repo();
    }

    /// 获取。
    pub fn op_fetch(&mut self) {
        if let Some(p) = self.begin_op("git fetch --all --prune") {
            let r = ops::fetch(&p);
            self.finish_op(r, "获取完成");
        }
    }

    /// 拉取。
    pub fn op_pull(&mut self) {
        if let Some(p) = self.begin_op("git pull --no-rebase") {
            let r = ops::pull(&p);
            self.finish_op(r, "拉取完成");
        }
    }

    /// 推送。
    pub fn op_push(&mut self) {
        if let Some(p) = self.begin_op("git push") {
            let r = ops::push(&p);
            self.finish_op(r, "推送完成");
        }
    }

    /// 贮藏。
    pub fn op_stash(&mut self) {
        if let Some(p) = self.begin_op("git stash push -u") {
            let r = ops::stash(&p);
            self.finish_op(r, "已贮藏");
        }
    }

    /// 暂存全部。
    pub fn op_stage_all(&mut self) {
        if let Some(p) = self.begin_op("git add -A") {
            let r = ops::stage_all(&p);
            self.finish_op(r, "已暂存全部改动");
        }
    }

    /// 用输入框里的信息提交。
    pub fn op_commit(&mut self) {
        let msg = self.commit_message.clone();
        if msg.trim().is_empty() {
            self.flash("提交信息不能为空");
            self.push_log(LogKind::Err, "提交信息不能为空");
            return;
        }
        let cmd = format!("git commit -m '{}'", msg.trim());
        if let Some(p) = self.begin_op(&cmd) {
            let r = ops::commit(&p, &msg, true);
            let ok = r.is_ok();
            self.finish_op(r, "提交完成");
            if ok {
                self.commit_message.clear();
            }
        }
    }

    /// 新建并检出分支。
    pub fn op_create_branch(&mut self) {
        let name = self.branch_input.clone();
        if name.trim().is_empty() {
            self.flash("分支名为空");
            return;
        }
        if let Some(p) = self.begin_op(&format!("git checkout -b {}", name.trim())) {
            match gitbranch::create_branch(&p, &name, None, true) {
                Ok(()) => {
                    self.push_log(LogKind::Ok, format!("已新建并检出 {}", name.trim()));
                    self.branch_input.clear();
                    self.flash(format!("已切换 {}", name.trim()));
                }
                Err(e) => self.push_log(LogKind::Err, e.to_string()),
            }
            self.reload_repo();
        }
    }

    /// 检出分支。
    pub fn op_checkout(&mut self, name: &str) {
        if let Some(p) = self.begin_op(&format!("git checkout {name}")) {
            match gitbranch::checkout_branch(&p, name) {
                Ok(()) => {
                    self.push_log(LogKind::Ok, format!("已检出 {name}"));
                    self.flash(format!("已切换到 {name}"));
                }
                Err(e) => self.push_log(LogKind::Err, e.to_string()),
            }
            self.reload_repo();
        }
    }

    /// 删除分支。
    pub fn op_delete_branch(&mut self, name: &str) {
        if let Some(p) = self.begin_op(&format!("git branch -d {name}")) {
            match gitbranch::delete_branch(&p, name, false) {
                Ok(()) => {
                    self.push_log(LogKind::Ok, format!("已删除 {name}"));
                    self.flash(format!("已删除 {name}"));
                }
                Err(e) => self.push_log(LogKind::Err, e.to_string()),
            }
            self.reload_repo();
        }
    }

    /// 扫描目录，记录发现但**不自动写入配置**（由用户确认后再加）。
    pub fn op_scan(&mut self) {
        let root = std::path::PathBuf::from(self.scan_dir.trim());
        if self.scan_dir.trim().is_empty() {
            self.flash("请填写要扫描的目录");
            return;
        }
        self.push_log(
            LogKind::Cmd,
            format!("扫描 {}（最大深度 3）", root.display()),
        );
        let opts = discover::ScanOptions::default();
        let found = discover::scan(&root, &opts);
        self.scanned = found.iter().map(|f| f.dir_name()).collect();
        if found.is_empty() {
            self.push_log(LogKind::Out, "未发现 git 仓库".to_string());
            self.flash("未发现仓库");
            return;
        }
        let platforms = self.cfg.platforms.clone();
        for f in &found {
            let plat = f
                .platform(&platforms)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "未匹配平台".to_string());
            self.push_log(LogKind::Out, format!("{}  [{}]", f.path.display(), plat));
        }
        self.push_log(
            LogKind::Ok,
            format!("共发现 {} 个仓库（尚未加入配置）", found.len()),
        );
        self.flash(format!("发现 {} 个仓库", found.len()));
    }

    /// 切换项目身份：只改这个项目，并立即写 `.git/config`。
    pub fn switch_identity(&mut self, repo_id: &str, identity_id: &str) {
        let Some(idx) = self.cfg.repositories.iter().position(|r| r.id == repo_id) else {
            self.push_log(LogKind::Err, "找不到项目");
            return;
        };
        let repo = self.cfg.repositories[idx].clone();
        let Some(identity) = self.cfg.identity(identity_id).cloned() else {
            self.push_log(LogKind::Err, format!("找不到身份 {identity_id}"));
            return;
        };
        if identity.platform_id != repo.platform_id {
            self.push_log(
                LogKind::Err,
                format!(
                    "身份属于平台 {}，与项目的平台 {} 不一致，已拒绝",
                    identity.platform_id, repo.platform_id
                ),
            );
            return;
        }

        self.cfg.repositories[idx].identity_id = Some(identity_id.to_string());
        self.push_log(
            LogKind::Cmd,
            format!("# {} 切换到 {}", repo.dir_name(), identity.label),
        );
        self.write_repo(idx);
    }

    /// 保存项目覆盖并写盘。
    pub fn apply_override(&mut self, repo_id: &str) {
        let Some(idx) = self.cfg.repositories.iter().position(|r| r.id == repo_id) else {
            return;
        };
        let buf = self.overrides.get(repo_id).cloned().unwrap_or_default();
        let name = buf.name.trim().to_string();
        let email = buf.email.trim().to_string();
        self.cfg.repositories[idx].identity_override = if buf.on {
            IdentityOverride {
                enabled: true,
                user_name: (!name.is_empty()).then_some(name),
                user_email: (!email.is_empty()).then_some(email),
            }
        } else {
            IdentityOverride::none()
        };
        self.push_log(LogKind::Cmd, "# 更新项目覆盖");
        self.write_repo(idx);
    }

    /// 清除项目本地身份，回到跟随全局。
    pub fn clear_local_identity(&mut self, repo_id: &str) {
        let Some(repo) = self.cfg.repository(repo_id).cloned() else {
            return;
        };
        self.push_log(
            LogKind::Cmd,
            format!(
                "git -C {} config --local --unset user.name",
                repo.path.display()
            ),
        );
        match gitcfg::clear_identity(&repo.path) {
            Ok(()) => self.push_log(
                LogKind::Ok,
                format!("已清除 {} 的本地身份，回到跟随全局", repo.dir_name()),
            ),
            Err(e) => self.push_log(LogKind::Err, format!("清除失败：{e}")),
        }
    }

    /// 把指定项目当前的身份（含覆盖）写进 `.git/config`。
    fn write_repo(&mut self, idx: usize) {
        let repo = self.cfg.repositories[idx].clone();
        let Some(identity) = repo
            .identity_id
            .as_deref()
            .and_then(|id| self.cfg.identity(id))
            .cloned()
        else {
            self.push_log(LogKind::Err, "该项目尚未选择身份");
            return;
        };
        let effective = repo.effective_identity(&identity);

        self.push_log(
            LogKind::Cmd,
            format!(
                "git -C {} config --local user.name \"{}\"",
                repo.path.display(),
                effective.user_name
            ),
        );
        match gitcfg::apply_identity(&repo.path, &identity, &effective) {
            Ok(()) => {
                self.push_log(
                    LogKind::Ok,
                    format!("已写入 {}/.git/config", repo.path.display()),
                );
                self.push_log(
                    LogKind::Out,
                    format!(
                        "user.email = {}（来源：{}）",
                        effective.user_email,
                        effective.email_source.label()
                    ),
                );
                match self.cfg.save() {
                    Ok(()) => self.push_log(LogKind::Out, "配置文件已更新"),
                    Err(e) => self.push_log(LogKind::Err, format!("保存配置失败：{e}")),
                }
                self.flash(format!("{} → {}", repo.dir_name(), identity.label));
            }
            Err(e) => self.push_log(LogKind::Err, format!("写入失败：{e}")),
        }
    }

    // ---- 文件 / 编辑器 ----

    /// 打开仓库内某个文件到编辑器；已打开同一个文件时先按未保存修改提示。
    ///
    /// 未跟踪文件的 blame 会失败，这里只记下原因，不阻断编辑。
    pub fn open_doc(&mut self, rel: &str) {
        let Some(path) = self.repo_path() else {
            self.flash("请先选择项目");
            return;
        };
        if let Some(doc) = &self.doc
            && doc.dirty()
            && doc.path != rel
        {
            self.flash("当前文件有未保存修改，请先保存或放弃");
            return;
        }
        self.push_log(LogKind::Cmd, format!("打开 {}（{}）", rel, path.display()));
        let content = match gitfile::read_file(&path, rel) {
            Ok(c) => c,
            Err(e) => {
                self.push_log(LogKind::Err, format!("打开失败：{e}"));
                return;
            }
        };
        let lang = Language::from_path(std::path::Path::new(rel));
        let mut doc = OpenDoc {
            path: rel.to_string(),
            text: content.text.clone(),
            saved_text: content.text,
            lang,
            diags: Vec::new(),
            tokens: Vec::new(),
            blame: Vec::new(),
            blame_error: None,
        };
        doc.reanalyze();
        match gitfile::blame(&path, rel) {
            Ok(b) => doc.blame = b,
            Err(e) => doc.blame_error = Some(e.to_string()),
        }
        self.push_log(
            LogKind::Out,
            format!(
                "{} 行 · {} · {} 条语法提示{}",
                doc.text.lines().count(),
                lang.label(),
                doc.diags.len(),
                match &doc.blame_error {
                    Some(_) => " · blame 不可用（未跟踪）",
                    None => "",
                }
            ),
        );
        self.doc = Some(doc);
        self.tab = DetailTab::Editor;
    }

    /// 保存当前文档；磁盘写成功后刷新状态与 diff。
    pub fn save_doc(&mut self) {
        let Some(path) = self.repo_path() else {
            return;
        };
        let Some(doc) = self.doc.clone() else {
            return;
        };
        if !doc.dirty() {
            self.flash("没有需要保存的修改");
            return;
        }
        self.push_log(
            LogKind::Cmd,
            format!("写入 {}/{}", path.display(), doc.path),
        );
        match gitfile::write_file(&path, &doc.path, &doc.text) {
            Ok(()) => {
                self.push_log(
                    LogKind::Ok,
                    format!("已保存 {}（{} 字节）", doc.path, doc.text.len()),
                );
                self.flash(format!("已保存 {}", doc.path));
                self.reload_repo();
                if let Some(d) = &mut self.doc {
                    d.saved_text = d.text.clone();
                    d.reanalyze();
                }
            }
            Err(e) => self.push_log(LogKind::Err, format!("保存失败：{e}")),
        }
    }

    /// 放弃未保存修改，回到磁盘内容。
    pub fn revert_doc(&mut self) {
        let Some(doc) = &mut self.doc else {
            return;
        };
        if !doc.dirty() {
            return;
        }
        doc.text = doc.saved_text.clone();
        doc.reanalyze();
        self.flash("已放弃修改");
    }

    /// 读取仓库 README（本地文件，不是写死的字符串）。
    pub fn load_readme(&mut self) {
        self.readme = None;
        self.readme_error = None;
        let Some(path) = self.repo_path() else {
            return;
        };
        self.push_log(LogKind::Cmd, format!("查找 {} 下的 README", path.display()));
        match gitfile::find_readme(&path) {
            Some(rel) => match gitfile::read_file(&path, &rel) {
                Ok(c) => {
                    let lines: Vec<String> = c.text.lines().map(|l| l.to_string()).collect();
                    self.push_log(LogKind::Out, format!("读取 {rel}（{} 行）", lines.len()));
                    self.readme = Some((rel, lines));
                }
                Err(e) => {
                    self.readme_error = Some(e.to_string());
                    self.push_log(LogKind::Err, format!("读取 README 失败：{e}"));
                }
            },
            None => {
                self.readme_error = Some("仓库里没有找到 README".to_string());
                self.push_log(
                    LogKind::Out,
                    "未找到 README（已查根目录与 docs/）".to_string(),
                );
            }
        }
    }

    // ---- 克隆 ----

    /// 打开克隆弹窗，按当前平台预填身份与目录。
    pub fn open_clone(&mut self) {
        let platform = self.current_platform().map(|p| p.id.clone());
        let identity = platform
            .as_deref()
            .and_then(|pid| self.identities_of(pid).first().map(|i| i.id.clone()));
        self.clone_buf = CloneBuf {
            open: true,
            url: String::new(),
            parent: self.scan_dir.clone(),
            dir_name: String::new(),
            platform_hint: String::new(),
            identity_id: identity.unwrap_or_default(),
            error: None,
            busy: false,
        };
    }

    /// 根据输入的地址推导平台与目录名（供弹窗实时提示）。
    pub fn refresh_clone_hint(&mut self) {
        let url = self.clone_buf.url.trim().to_string();
        if url.is_empty() {
            self.clone_buf.platform_hint.clear();
            return;
        }
        match RemoteUrl::parse(&url) {
            Ok(parsed) => {
                let host = parsed.host.clone();
                let matched = self
                    .cfg
                    .platforms
                    .iter()
                    .find(|p| p.matches_host(&host))
                    .map(|p| p.id.clone());
                self.clone_buf.platform_hint = match &matched {
                    Some(pid) => {
                        // 平台换了就换该平台的第一个身份，避免身份与平台错配
                        if self.clone_buf.identity_id.is_empty()
                            || self
                                .cfg
                                .identity(&self.clone_buf.identity_id)
                                .map(|i| i.platform_id.as_str())
                                != Some(pid.as_str())
                        {
                            self.clone_buf.identity_id = self
                                .identities_of(pid)
                                .first()
                                .map(|i| i.id.clone())
                                .unwrap_or_default();
                        }
                        self.cfg
                            .platform(pid)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| host.clone())
                    }
                    None => {
                        self.clone_buf.identity_id.clear();
                        format!("未匹配平台（{host}）")
                    }
                };
            }
            Err(_) => {
                self.clone_buf.platform_hint = "地址无法解析".to_string();
                self.clone_buf.identity_id.clear();
            }
        }
    }

    /// 执行克隆，并把选中的身份立刻写进新仓库。
    pub fn run_clone(&mut self) {
        if self.clone_buf.busy {
            return;
        }
        let url = self.clone_buf.url.trim().to_string();
        if url.is_empty() {
            self.clone_buf.error = Some("请填写仓库地址".to_string());
            return;
        }
        let parent = std::path::PathBuf::from(self.clone_buf.parent.trim());
        if self.clone_buf.parent.trim().is_empty() {
            self.clone_buf.error = Some("请填写克隆到哪个目录".to_string());
            return;
        }

        let mut req = CloneRequest::new(url.clone(), parent.clone());
        if !self.clone_buf.dir_name.trim().is_empty() {
            req = req.with_dir_name(self.clone_buf.dir_name.trim());
        }

        // 先算目标路径，冲突立即反馈，不让 git 白跑一趟
        let target = match req.target_path() {
            Ok(t) => t,
            Err(e) => {
                self.clone_buf.error = Some(e.to_string());
                return;
            }
        };
        if target.exists()
            && std::fs::read_dir(&target)
                .map(|mut it| it.next().is_some())
                .unwrap_or(true)
        {
            self.clone_buf.error = Some(format!("{} 已存在且非空", target.display()));
            return;
        }

        let identity = self
            .cfg
            .identity(self.clone_buf.identity_id.trim())
            .cloned();
        let effective =
            identity
                .as_ref()
                .map(|i| git_switch::model::repository::EffectiveIdentity {
                    user_name: i.user_name.clone(),
                    user_email: i.user_email.clone(),
                    name_source: git_switch::model::repository::ValueSource::Identity,
                    email_source: git_switch::model::repository::ValueSource::Identity,
                });

        self.push_log(
            LogKind::Cmd,
            format!("git clone {} {}", url, target.display()),
        );
        let outcome = match (&identity, &effective) {
            (Some(i), Some(e)) => clone::clone_repo(&req, Some((i, e))),
            _ => clone::clone_repo(&req, None),
        };

        match outcome {
            Ok(o) => {
                for line in o.output.lines() {
                    if !line.trim().is_empty() {
                        self.push_log(LogKind::Out, line.to_string());
                    }
                }
                let dir_name = o
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.push_log(LogKind::Ok, format!("已克隆到 {}", o.path.display()));
                if identity.is_some() {
                    self.push_log(LogKind::Ok, "身份已写入新仓库的 .git/config");
                }

                // 加入配置并选中
                let platform_id = self
                    .cfg
                    .platforms
                    .iter()
                    .find(|p| p.matches_host(&url_host(&url)))
                    .map(|p| p.id.clone());
                if let Some(pid) = platform_id {
                    let id = format!("r-{dir_name}");
                    let mut repo =
                        Repository::new(id.clone(), pid.clone(), o.path.clone(), url.clone());
                    repo.identity_id = identity.as_ref().map(|i| i.id.clone());
                    if self.cfg.repository(&id).is_none() {
                        self.cfg.repositories.push(repo);
                        match self.cfg.save() {
                            Ok(()) => self.push_log(LogKind::Out, "已加入项目列表并保存配置"),
                            Err(e) => self.push_log(LogKind::Err, format!("保存配置失败：{e}")),
                        }
                    }
                    self.platform = Some(pid);
                    self.clone_buf.open = false;
                    self.clone_buf.error = None;
                    self.select_repo(&id);
                    self.flash(format!("已克隆 {dir_name}"));
                } else {
                    self.clone_buf.error =
                        Some("克隆成功，但地址未匹配到平台，未加入项目列表".to_string());
                    self.push_log(LogKind::Err, "地址未匹配到平台，仓库已在磁盘上但未加入配置");
                }
            }
            Err(e) => {
                let msg = match &e {
                    CloneError::TargetNotEmpty(_) | CloneError::BadUrl(_) => e.to_string(),
                    _ => e.to_string(),
                };
                self.push_log(LogKind::Err, format!("克隆失败：{msg}"));
                self.clone_buf.error = Some(msg);
            }
        }
    }

    /// 追加一行日志。
    pub fn push_log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.log.push(LogEntry::new(kind, text));
        if self.log.len() > 500 {
            self.log.drain(0..100);
        }
    }

    /// 顶部提示。
    pub fn flash(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), 2.5));
    }

    /// 每帧推进提示倒计时。
    pub fn tick(&mut self, dt: f32) {
        if let Some((_, left)) = &mut self.toast {
            *left -= dt;
            if *left <= 0.0 {
                self.toast = None;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dt = ctx.input(|i| i.stable_dt);
        self.tick(dt);
        ui::draw(ctx, self);
        // 提示条需要持续重绘才能倒计时
        if self.toast.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// 取地址的 host，失败时返回空串。
fn url_host(url: &str) -> String {
    RemoteUrl::parse(url).map(|u| u.host).unwrap_or_default()
}

/// 默认扫描目录：用户主目录下的 WorkSpace，不存在则退回主目录。
fn default_scan_dir() -> String {
    match dirs::home_dir() {
        Some(home) => {
            let workspace = home.join("WorkSpace");
            if workspace.is_dir() {
                workspace.display().to_string()
            } else {
                home.display().to_string()
            }
        }
        None => String::new(),
    }
}
