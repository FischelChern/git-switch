//! 三栏与底栏的具体面板。
//!
//! 首版集中在一个文件里，函数按面板划分；后续按 `AGENTS.md` 6.2 拆成
//! `sidebar.rs` / `projects.rs` / `detail.rs` / `terminal.rs`。

use eframe::egui;

use crate::app::{App, LogKind};
use crate::ui::theme::*;

/// 左侧：平台 · 身份树。
pub fn sidebar(ctx: &egui::Context, app: &mut App) {
    egui::SidePanel::left("sidebar")
        .exact_width(240.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("平台 · 身份").color(DIM).size(11.0));
            });
            ui.separator();

            let platforms = app.cfg.platforms.clone();
            let mut pending_platform: Option<String> = None;
            let mut pending_identity: Option<(String, String)> = None;
            let current_platform = app.platform.clone();
            let current_identity = app
                .current_repo()
                .cloned()
                .and_then(|r| r.identity_id.map(|i| (r.id, i)));

            egui::ScrollArea::vertical()
                .id_salt("sidebar_scroll")
                .show(ui, |ui| {
                    for p in &platforms {
                        let is_sel = current_platform.as_deref() == Some(p.id.as_str());
                        let expanded = app.expanded.contains(&p.id);
                        let arrow = if expanded { "▾" } else { "▸" };
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    is_sel,
                                    egui::RichText::new(format!("{arrow} {}", p.name)).size(12.5),
                                )
                                .clicked()
                            {
                                pending_platform = Some(p.id.clone());
                            }
                            if ui
                                .small_button(if expanded { "−" } else { "+" })
                                .on_hover_text("展开 / 收起")
                                .clicked()
                            {
                                if expanded {
                                    app.expanded.retain(|x| x != &p.id);
                                } else {
                                    app.expanded.push(p.id.clone());
                                }
                            }
                        });

                        if !expanded {
                            continue;
                        }
                        let identities = app.identities_of(&p.id);
                        if identities.is_empty() {
                            ui.label(
                                egui::RichText::new("    （该平台暂无身份）")
                                    .color(DIM)
                                    .size(11.0),
                            );
                        }
                        for id in identities {
                            let in_use = current_identity
                                .as_ref()
                                .map(|(_, iid)| iid == &id.id)
                                .unwrap_or(false);
                            let used = app.cfg.repositories_using(&id.id).len();
                            let text = format!(
                                "    {} · {}{}",
                                id.label,
                                id.auth.label(),
                                if in_use {
                                    "  ← 当前".to_string()
                                } else if used > 0 {
                                    format!("  （{used} 项目）")
                                } else {
                                    String::new()
                                }
                            );
                            let resp =
                                ui.selectable_label(in_use, egui::RichText::new(text).size(11.5));
                            if resp
                                .on_hover_text("点击：把这个身份应用到当前选中的项目")
                                .clicked()
                            {
                                pending_identity = Some((p.id.clone(), id.id.clone()));
                            }
                        }
                        ui.add_space(2.0);
                    }
                });

            ui.separator();
            if app.font_path.is_none() {
                ui.label(
                    egui::RichText::new("⚠ 未找到中文字体")
                        .color(ERR)
                        .size(11.0),
                );
            }
            ui.label(
                egui::RichText::new(format!("配置：{}", app.config_path))
                    .color(DIM)
                    .size(10.0),
            );

            if let Some(pid) = pending_platform {
                app.select_platform(&pid);
            }
            if let Some((pid, iid)) = pending_identity {
                let Some(repo) = app.current_repo().cloned() else {
                    app.flash("请先在中间列表选择一个项目");
                    return;
                };
                if repo.platform_id != pid {
                    app.flash("当前项目不属于该平台，请先选择该平台下的项目");
                    app.push_log(
                        LogKind::Err,
                        format!("平台不匹配：项目在 {}，身份在 {pid}", repo.platform_id),
                    );
                } else {
                    app.switch_identity(&repo.id, &iid);
                }
            }
        });
}

/// 中间：当前平台下的全部项目。
pub fn projects(ctx: &egui::Context, app: &mut App) {
    egui::SidePanel::left("projects")
        .exact_width(280.0)
        .resizable(false)
        .show(ctx, |ui| {
            let platform_name = app
                .current_platform()
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "未选择平台".to_string());
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{platform_name} · 项目"))
                        .color(DIM)
                        .size(11.0),
                );
            });
            ui.separator();

            let repos = app.repos_of_current_platform();
            if repos.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("该平台下暂无项目")
                            .color(DIM)
                            .size(12.0),
                    );
                    ui.label(
                        egui::RichText::new("克隆功能尚未实现，可手工把仓库加进配置")
                            .color(DIM)
                            .size(10.5),
                    );
                });
                return;
            }

            let current = app.repo.clone();
            let mut pending: Option<String> = None;
            egui::ScrollArea::vertical()
                .id_salt("projects_scroll")
                .show(ui, |ui| {
                    for r in &repos {
                        let selected = current.as_deref() == Some(r.id.as_str());
                        let identity_label = app
                            .identity_of(r)
                            .map(|i| i.label.clone())
                            .unwrap_or_else(|| "未绑定身份".to_string());
                        let resp = egui::Frame::group(ui.style())
                            .fill(if selected { PANEL_2 } else { PANEL })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if selected { ACCENT } else { LINE },
                            ))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(r.dir_name()).strong().size(12.5));
                                    ui.label(
                                        egui::RichText::new(&r.remote_url).color(DIM).size(10.0),
                                    );
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("◈ {}", identity_label))
                                                .color(if selected { ACCENT } else { MUTED })
                                                .size(10.5),
                                        );
                                        if let Some(ov) = Some(&r.identity_override) {
                                            if ov.is_active() {
                                                ui.label(
                                                    egui::RichText::new("· 含覆盖")
                                                        .color(WARN)
                                                        .size(10.0),
                                                );
                                            }
                                        }
                                    });
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        if resp.clicked() {
                            pending = Some(r.id.clone());
                        }
                        ui.add_space(4.0);
                    }
                });

            if let Some(id) = pending {
                app.select_repo(&id);
            }
        });
}

/// 右侧：项目详情（平台锁定 / 身份可切 / 项目覆盖 / 写入范围）。
pub fn detail(ctx: &egui::Context, app: &mut App) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(repo) = app.current_repo().cloned() else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("请选择一个项目").color(DIM));
            });
            return;
        };

        let platform = app.cfg.platform(&repo.platform_id).cloned();
        let identity = app.identity_of(&repo).cloned();

        // 标题
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(repo.dir_name()).strong().size(15.0));
            if let Some(p) = &platform {
                ui.label(egui::RichText::new(&p.name).color(ACCENT).size(11.0));
            }
            ui.label(
                egui::RichText::new(
                    repo.remote()
                        .map(|r| r.protocol.as_str().to_string())
                        .unwrap_or_else(|| "—".to_string()),
                )
                .color(MUTED)
                .size(11.0),
            );
        });
        ui.label(
            egui::RichText::new(repo.path.display().to_string())
                .color(DIM)
                .size(10.5),
        );
        ui.label(egui::RichText::new(&repo.remote_url).color(DIM).size(10.5));
        ui.add_space(8.0);

        // 平台（锁定）+ 身份（可切）
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("平台 · 由远程地址决定")
                        .color(DIM)
                        .size(10.0),
                );
                let name = platform
                    .as_ref()
                    .map(|p| p.name.clone())
                    .unwrap_or_else(|| "未识别".to_string());
                ui.add_enabled(
                    false,
                    egui::Button::new(egui::RichText::new(format!("🔒 {name}")).size(12.0)),
                )
                .on_disabled_hover_text("平台由仓库远程地址决定，不可切换");
            });

            ui.add_space(12.0);

            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("git 身份 · 只作用于本项目")
                        .color(DIM)
                        .size(10.0),
                );
                let identities = app.identities_of(&repo.platform_id);
                let current_label = identity
                    .as_ref()
                    .map(|i| i.label.clone())
                    .unwrap_or_else(|| "未选择身份".to_string());
                let mut chosen = repo.identity_id.clone();

                egui::ComboBox::from_id_salt("identity_combo")
                    .selected_text(egui::RichText::new(current_label).size(12.0))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for id in &identities {
                            let label = format!("{} · {}", id.label, id.auth.label());
                            ui.selectable_value(&mut chosen, Some(id.id.clone()), label);
                        }
                    });

                if chosen != repo.identity_id {
                    if let Some(new_id) = chosen {
                        app.switch_identity(&repo.id, &new_id);
                    }
                }
            });
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("重新应用到此仓库").clicked() {
                if let Some(id) = repo.identity_id.clone() {
                    app.switch_identity(&repo.id, &id);
                } else {
                    app.flash("请先选择身份");
                }
            }
            if ui.button("清除本地身份（跟随全局）").clicked() {
                app.clear_local_identity(&repo.id);
            }
        });

        ui.separator();

        // 项目覆盖
        ui.label(
            egui::RichText::new("项目覆盖 · 优先级高于身份默认值")
                .color(DIM)
                .size(10.0),
        );
        let repo_id = repo.id.clone();
        let identity_name = identity
            .as_ref()
            .map(|i| i.user_name.clone())
            .unwrap_or_default();
        let identity_email = identity
            .as_ref()
            .map(|i| i.user_email.clone())
            .unwrap_or_default();

        let mut apply_clicked = false;
        {
            let buf = app.override_buf(&repo_id);
            ui.horizontal(|ui| {
                ui.checkbox(&mut buf.on, "启用覆盖");
                ui.add_enabled(
                    buf.on,
                    egui::TextEdit::singleline(&mut buf.name)
                        .hint_text(if buf.on {
                            "项目专用 user.name"
                        } else {
                            "继承身份默认"
                        })
                        .desired_width(180.0),
                );
                ui.add_enabled(
                    buf.on,
                    egui::TextEdit::singleline(&mut buf.email)
                        .hint_text(if buf.on {
                            "项目专用 user.email"
                        } else {
                            "继承身份默认"
                        })
                        .desired_width(220.0),
                );
                if ui.button("应用覆盖").clicked() {
                    apply_clicked = true;
                }
            });

            // 生效值预览
            let eff_name = if buf.on && !buf.name.trim().is_empty() {
                buf.name.trim().to_string()
            } else {
                identity_name.clone()
            };
            let eff_email = if buf.on && !buf.email.trim().is_empty() {
                buf.email.trim().to_string()
            } else {
                identity_email.clone()
            };
            let source = if buf.on && (!buf.name.trim().is_empty() || !buf.email.trim().is_empty())
            {
                "项目覆盖"
            } else {
                "身份默认"
            };
            ui.label(
                egui::RichText::new(format!("生效：{eff_name} <{eff_email}>（来源：{source}）"))
                    .color(if source == "项目覆盖" { OK } else { MUTED })
                    .size(11.0),
            );
        }
        if apply_clicked {
            app.apply_override(&repo_id);
        }

        ui.add_space(6.0);
        ui.separator();

        // 写入范围提示
        egui::Frame::group(ui.style())
            .fill(PANEL)
            .stroke(egui::Stroke::new(1.0_f32, LINE))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "写入范围：仅 {}/.git/config，不改动 ~/.gitconfig",
                        repo.path.display()
                    ))
                    .color(WARN)
                    .size(11.0),
                );
            });
    });
}

/// 底部：终端日志。
pub fn terminal(ctx: &egui::Context, app: &mut App) {
    egui::TopBottomPanel::bottom("terminal")
        .resizable(true)
        .default_height(170.0)
        .min_height(80.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("终端").color(MUTED).size(11.0));
                ui.label(
                    egui::RichText::new(format!("{} 行", app.log.len()))
                        .color(DIM)
                        .size(10.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("清空").clicked() {
                        app.log.clear();
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("terminal_scroll")
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in &app.log {
                        let (color, prefix) = match entry.kind {
                            LogKind::Cmd => (OK, "$ "),
                            LogKind::Out => (MUTED, "  "),
                            LogKind::Ok => (OK, "  "),
                            LogKind::Err => (ERR, "  "),
                        };
                        ui.label(
                            egui::RichText::new(format!("{prefix}{}", entry.text))
                                .color(color)
                                .size(11.0)
                                .monospace(),
                        );
                    }
                });
        });
}
