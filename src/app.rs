//! 应用状态机：持有配置、当前选择、终端日志与提示。
//!
//! 这里**不做渲染**（渲染在 `crate::ui`），也**不直接拼命令**
//! （写盘统一走 `git_switch::git::config`）。

use std::collections::HashMap;

use eframe::egui;

use git_switch::git::config as gitcfg;
use git_switch::model::identity::Identity;
use git_switch::model::repository::{IdentityOverride, Repository};
use git_switch::store::Config;

use crate::ui;

/// 日志行的类型，决定颜色。
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

        Self {
            cfg,
            platform,
            repo,
            overrides: HashMap::new(),
            expanded,
            log,
            toast: None,
            config_path: cfg_path,
            font_path,
        }
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
