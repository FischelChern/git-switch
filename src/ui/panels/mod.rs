//! 三栏与底栏的具体面板。
//!
//! 首版集中在一个文件里，函数按面板划分；后续按 `AGENTS.md` 6.2 拆成
//! `sidebar.rs` / `projects.rs` / `detail.rs` / `terminal.rs`。

pub mod editor;

use eframe::egui;

use git_switch::model::repository::Repository;

use crate::app::{App, DetailTab, LogKind};
use crate::ui::theme::*;
use git_switch::git::ChangeStatus;

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
                            let cred = app.cred_of(&id);
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
                            ui.horizontal(|ui| {
                                ui.add_space(4.0);
                                // 凭据状态徽标：状态来自真实钥匙串查询
                                ui.label(
                                    egui::RichText::new(cred.glyph())
                                        .monospace()
                                        .size(11.0)
                                        .color(cred_color(cred)),
                                )
                                .on_hover_text(cred.label());
                                let resp = ui
                                    .selectable_label(in_use, egui::RichText::new(text).size(11.5));
                                if resp
                                    .on_hover_text("点击：把这个身份应用到当前选中的项目")
                                    .clicked()
                                {
                                    pending_identity = Some((p.id.clone(), id.id.clone()));
                                }
                            });
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

/// 右侧：项目详情。顶部固定信息 + tab（概览 / git / 文件）。
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
            if let Some(status) = &app.status {
                if let Some(branch) = &status.branch {
                    ui.label(
                        egui::RichText::new(format!("◈ {branch}"))
                            .color(MUTED)
                            .size(11.0),
                    );
                }
                if status.ahead > 0 {
                    ui.label(
                        egui::RichText::new(format!("↑{}", status.ahead))
                            .color(OK)
                            .size(10.5),
                    );
                }
                if status.behind > 0 {
                    ui.label(
                        egui::RichText::new(format!("↓{}", status.behind))
                            .color(WARN)
                            .size(10.5),
                    );
                }
                if !status.files.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("{} 变更", status.files.len()))
                            .color(WARN)
                            .size(10.5),
                    );
                }
            }
        });
        ui.label(
            egui::RichText::new(repo.path.display().to_string())
                .color(DIM)
                .size(10.5),
        );
        ui.label(egui::RichText::new(&repo.remote_url).color(DIM).size(10.5));

        // tab 栏
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for t in DetailTab::all() {
                if ui.selectable_label(app.tab == t, t.label()).clicked() {
                    app.tab = t;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("重新读取").clicked() {
                    app.reload_repo();
                }
            });
        });
        ui.separator();

        match app.tab {
            DetailTab::Overview => overview(ui, app, &repo),
            DetailTab::Git => git_tab(ui, app),
            DetailTab::Files => files_tab(ui, app),
            DetailTab::Diff => diff_tab(ui, app),
            DetailTab::Readme => editor::readme_tab(ui, app),
            DetailTab::Editor => editor::editor_tab(ui, app),
        }
    });
}

/// 概览：平台锁定、身份切换、项目覆盖、写入范围。
fn overview(ui: &mut egui::Ui, app: &mut App, repo: &Repository) {
    let platform = app.cfg.platform(&repo.platform_id).cloned();
    let identity = app.identity_of(repo).cloned();

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

            // 凭据状态：来自真实钥匙串查询，不做「已保存」的默认假设
            if let Some(id) = &identity {
                let cred = app.cred_of(id);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{} {}", cred.glyph(), cred.label()))
                            .color(cred_color(cred))
                            .size(10.5),
                    );
                });
            }
        });

        ui.add_space(12.0);

        ui.vertical(|ui| {
            ui.label(egui::RichText::new("认证").color(DIM).size(10.0));
            if ui
                .button("测试认证")
                .on_hover_text("本地自洽性检查：协议与认证方式、钥匙串凭据、未做网络探测时明确标注")
                .clicked()
            {
                app.test_auth();
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
        let source = if buf.on && (!buf.name.trim().is_empty() || !buf.email.trim().is_empty()) {
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
}

/// 泳道配色。
const LANE_COLORS: [egui::Color32; 6] = [
    ACCENT,
    WARN,
    OK,
    egui::Color32::from_rgb(0xc7, 0x92, 0xea),
    egui::Color32::from_rgb(0xff, 0x8f, 0x6b),
    egui::Color32::from_rgb(0x4e, 0xc9, 0xb0),
];

/// 提交图。
fn git_tab(ui: &mut egui::Ui, app: &mut App) {
    let Some(graph) = app.graph.clone() else {
        ui.add_space(16.0);
        let msg = app
            .graph_error
            .clone()
            .unwrap_or_else(|| "没有提交记录".to_string());
        ui.label(egui::RichText::new(msg).color(DIM).size(12.0));
        return;
    };

    let lane_w = 14.0_f32;
    let row_h = 26.0_f32;
    let graph_w = graph.layout.lane_count as f32 * lane_w + 16.0;
    let lane_count = graph.layout.lane_count;

    branch_bar(ui, app);

    ui.label(
        egui::RichText::new(format!(
            "{} 条提交 · {} 条泳道",
            graph.commits.len(),
            lane_count
        ))
        .color(DIM)
        .size(10.0),
    );
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("git_graph_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (commit, row) in graph.commits.iter().zip(graph.layout.rows.iter()) {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(graph_w, row_h), egui::Sense::hover());
                    let painter = ui.painter_at(rect);
                    let x = |l: usize| rect.left() + 8.0 + l as f32 * lane_w;
                    let color = |l: usize| LANE_COLORS[l % LANE_COLORS.len()];
                    let mid = rect.center().y;

                    // 与本行无关的贯穿线
                    for l in 0..lane_count {
                        if l == row.lane {
                            continue;
                        }
                        if l < row.lanes_before && l < row.lanes_after {
                            painter.line_segment(
                                [
                                    egui::pos2(x(l), rect.top()),
                                    egui::pos2(x(l), rect.bottom()),
                                ],
                                egui::Stroke::new(1.4_f32, color(l)),
                            );
                        }
                    }
                    // 侧分支汇入
                    for &l in &row.merges_in {
                        painter.line_segment(
                            [egui::pos2(x(l), rect.top()), egui::pos2(x(row.lane), mid)],
                            egui::Stroke::new(1.4_f32, color(l)),
                        );
                    }
                    // 本提交自己的竖线
                    if row.lanes_before > row.lane {
                        painter.line_segment(
                            [
                                egui::pos2(x(row.lane), rect.top()),
                                egui::pos2(x(row.lane), mid),
                            ],
                            egui::Stroke::new(1.6_f32, color(row.lane)),
                        );
                    }
                    // 到父提交
                    for &p in &row.parent_lanes {
                        painter.line_segment(
                            [
                                egui::pos2(x(row.lane), mid),
                                egui::pos2(x(p), rect.bottom()),
                            ],
                            egui::Stroke::new(1.4_f32, color(p)),
                        );
                    }
                    // 提交点
                    let c = egui::pos2(x(row.lane), mid);
                    painter.circle_filled(c, 4.0, color(row.lane));
                    if commit.is_merge() {
                        painter.circle_filled(c, 1.8, BG);
                    }

                    ui.label(
                        egui::RichText::new(commit.short_id())
                            .monospace()
                            .size(10.5)
                            .color(DIM),
                    );
                    ui.label(egui::RichText::new(&commit.summary).size(12.0));
                    for r in &commit.refs {
                        ui.label(egui::RichText::new(r).color(ACCENT).size(9.5));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&commit.date).color(DIM).size(10.0));
                        ui.label(egui::RichText::new(&commit.author).color(DIM).size(10.0));
                    });
                });
            }
        });
}

/// 分支与标签操作栏。
fn branch_bar(ui: &mut egui::Ui, app: &mut App) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("分支").color(DIM).size(10.0));

        let branches = app.branches.clone();
        let mut pending_checkout: Option<String> = None;
        let mut pending_delete: Option<String> = None;

        for b in branches.iter().filter(|b| !b.remote).take(6) {
            let label = if b.current {
                format!("◈ {}", b.name)
            } else {
                b.name.clone()
            };
            let resp = ui.selectable_label(b.current, egui::RichText::new(label).size(11.0));
            if resp.clicked() && !b.current {
                pending_checkout = Some(b.name.clone());
            }
            if !b.current {
                resp.context_menu(|ui| {
                    if ui.button("删除该分支").clicked() {
                        pending_delete = Some(b.name.clone());
                        ui.close_menu();
                    }
                });
            }
        }
        if let Some(name) = pending_checkout {
            app.op_checkout(&name);
        }
        if let Some(name) = pending_delete {
            app.op_delete_branch(&name);
        }

        ui.add_space(6.0);
        ui.add(
            egui::TextEdit::singleline(&mut app.branch_input)
                .hint_text("新分支名")
                .desired_width(120.0),
        );
        if ui.small_button("新建并检出").clicked() {
            app.op_create_branch();
        }

        if !app.tags.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("标签").color(DIM).size(10.0));
            for t in app.tags.iter().take(4) {
                ui.label(egui::RichText::new(t).color(WARN).size(10.0));
            }
        }
    });
    ui.add_space(4.0);
    ui.separator();

    // 常用写操作
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("暂存全部").clicked() {
            app.op_stage_all();
        }
        if ui.small_button("提交").clicked() {
            app.op_commit();
        }
        if ui.small_button("获取").clicked() {
            app.op_fetch();
        }
        if ui.small_button("拉取").clicked() {
            app.op_pull();
        }
        if ui.small_button("推送").clicked() {
            app.op_push();
        }
        if ui.small_button("贮藏").clicked() {
            app.op_stash();
        }
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.commit_message)
                .hint_text("提交信息（不能为空）")
                .desired_width(ui.available_width() - 8.0),
        );
    });
    ui.add_space(2.0);
    ui.separator();
}

/// 差异视图。
fn diff_tab(ui: &mut egui::Ui, app: &mut App) {
    if app.diffs.is_empty() {
        ui.add_space(16.0);
        let msg = app
            .diff_error
            .clone()
            .unwrap_or_else(|| "工作区没有差异".to_string());
        ui.label(egui::RichText::new(msg).color(DIM).size(12.0));
        return;
    }

    ui.horizontal(|ui| {
        ui.selectable_value(&mut app.diff_side_by_side, true, "左右");
        ui.selectable_value(&mut app.diff_side_by_side, false, "上下");
        ui.label(
            egui::RichText::new(format!("{} 个文件", app.diffs.len()))
                .color(DIM)
                .size(10.0),
        );
    });
    ui.separator();

    let side = app.diff_side_by_side;
    egui::ScrollArea::both()
        .id_salt("diff_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for f in &app.diffs {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(f.display_path()).monospace().size(11.5));
                    ui.label(
                        egui::RichText::new(format!("+{}", f.additions))
                            .color(OK)
                            .size(10.5),
                    );
                    ui.label(
                        egui::RichText::new(format!("-{}", f.deletions))
                            .color(ERR)
                            .size(10.5),
                    );
                    if f.binary {
                        ui.label(egui::RichText::new("二进制").color(WARN).size(10.5));
                    }
                });

                if f.binary {
                    ui.label(
                        egui::RichText::new("（二进制文件不展示逐行差异）")
                            .color(DIM)
                            .size(10.5),
                    );
                    ui.separator();
                    continue;
                }

                if side {
                    let rows = git_switch::git::diff::to_side_by_side(&f.rows);
                    egui::Grid::new(format!("diff_side_{}", f.new_path))
                        .num_columns(2)
                        .striped(false)
                        .spacing([10.0, 1.0])
                        .show(ui, |ui| {
                            for r in &rows {
                                match &r.left {
                                    Some(l) => {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:>4} {}",
                                                l.line_no.unwrap_or(0),
                                                l.text
                                            ))
                                            .monospace()
                                            .size(10.5)
                                            .color(ERR),
                                        );
                                    }
                                    None => {
                                        ui.label(egui::RichText::new("").size(10.5));
                                    }
                                }
                                match &r.right {
                                    Some(l) => {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{:>4} {}",
                                                l.line_no.unwrap_or(0),
                                                l.text
                                            ))
                                            .monospace()
                                            .size(10.5)
                                            .color(OK),
                                        );
                                    }
                                    None => {
                                        ui.label(egui::RichText::new("").size(10.5));
                                    }
                                }
                                ui.end_row();
                            }
                        });
                } else {
                    for r in &f.rows {
                        let (text, color) = match r {
                            git_switch::git::DiffRow::Context { new, .. } => (
                                format!(" {:>4} {}", new.line_no.unwrap_or(0), new.text),
                                MUTED,
                            ),
                            git_switch::git::DiffRow::Removed(l) => {
                                (format!("-{:>4} {}", l.line_no.unwrap_or(0), l.text), ERR)
                            }
                            git_switch::git::DiffRow::Added(l) => {
                                (format!("+{:>4} {}", l.line_no.unwrap_or(0), l.text), OK)
                            }
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .monospace()
                                .size(10.5)
                                .color(color),
                        );
                    }
                }
                ui.separator();
            }
        });
}

/// 工作区变更列表。
fn files_tab(ui: &mut egui::Ui, app: &mut App) {
    let Some(status) = app.status.clone() else {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("未能读取工作区状态")
                .color(DIM)
                .size(12.0),
        );
        return;
    };

    if status.files.is_empty() {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("工作区干净，没有未提交改动")
                .color(OK)
                .size(12.0),
        );
        return;
    }

    ui.label(
        egui::RichText::new(format!("{} 个变更文件", status.files.len()))
            .color(DIM)
            .size(10.0),
    );
    ui.separator();

    let mut clicked: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt("files_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for f in &status.files {
                ui.horizontal(|ui| {
                    let color = match f.status {
                        ChangeStatus::Added => OK,
                        ChangeStatus::Modified => WARN,
                        ChangeStatus::Deleted => ERR,
                        ChangeStatus::Renamed | ChangeStatus::Copied => ACCENT,
                        ChangeStatus::Untracked => MUTED,
                        ChangeStatus::Conflicted => ERR,
                        ChangeStatus::TypeChanged => WARN,
                    };
                    ui.label(
                        egui::RichText::new(f.status.letter())
                            .monospace()
                            .size(11.0)
                            .color(color),
                    );
                    let open = ui
                        .add(
                            egui::Label::new(egui::RichText::new(&f.path).monospace().size(11.5))
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text("点击用编辑器打开");
                    if open.clicked() {
                        clicked = Some(f.path.clone());
                    }
                    if let Some(old) = &f.original_path {
                        ui.label(
                            egui::RichText::new(format!("← {old}"))
                                .color(DIM)
                                .size(10.5),
                        );
                    }
                    ui.label(
                        egui::RichText::new(f.status.label())
                            .color(color)
                            .size(10.0),
                    );
                });
            }
        });
    if let Some(path) = clicked {
        app.open_doc(&path);
    }
}

/// 凭据状态对应的颜色。
fn cred_color(status: git_switch::store::CredStatus) -> egui::Color32 {
    use git_switch::store::CredStatus as C;
    match status {
        C::Saved => OK,
        C::Missing => WARN,
        C::NotRequired => DIM,
        C::Unavailable => ERR,
    }
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
