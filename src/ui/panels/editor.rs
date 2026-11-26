//! README 与代码编辑器两个详情 tab。
//!
//! 这里只做渲染与事件转发：读文件、写文件、blame 全部由 [`App`] 侧调用
//! `git_switch::git::file` 完成。

use eframe::egui;

use git_switch::model::highlight::{Language, TokenKind};

use crate::app::{App, OpenDoc};
use crate::ui::theme::*;

/// README tab：展示仓库里真实的 README 文件。
pub fn readme_tab(ui: &mut egui::Ui, app: &mut App) {
    if app.readme.is_none() && app.readme_error.is_none() {
        app.load_readme();
    }

    ui.horizontal(|ui| {
        match &app.readme {
            Some((rel, lines)) => {
                ui.label(
                    egui::RichText::new(rel.clone())
                        .color(ACCENT)
                        .size(11.5)
                        .monospace(),
                );
                ui.label(
                    egui::RichText::new(format!("{} 行", lines.len()))
                        .color(DIM)
                        .size(10.0),
                );
            }
            None => {
                ui.label(egui::RichText::new("README").color(MUTED).size(11.0));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("重新读取").clicked() {
                app.load_readme();
            }
            if let Some((rel, _)) = app.readme.clone()
                && ui.small_button("用编辑器打开").clicked()
            {
                app.open_doc(&rel);
            }
        });
    });
    ui.separator();

    if let Some(err) = &app.readme_error {
        ui.add_space(12.0);
        ui.label(egui::RichText::new(err).color(WARN).size(12.0));
        ui.label(
            egui::RichText::new("已尝试根目录与 docs/、doc/、.github/ 下的 README.*")
                .color(DIM)
                .size(10.5),
        );
        return;
    }

    let Some((_, lines)) = &app.readme else {
        return;
    };
    let lines = lines.clone();
    let tokens = git_switch::model::highlight::highlight(&lines.join("\n"), Language::Markdown);
    let font = egui::FontId::monospace(11.5);

    egui::ScrollArea::both()
        .id_salt("readme_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, line) in lines.iter().enumerate() {
                let mut job = egui::text::LayoutJob::default();
                let mut cursor = 0usize;
                let line_tokens = tokens.get(idx).map(Vec::as_slice).unwrap_or(&[]);
                for t in line_tokens {
                    if t.start > cursor {
                        push_run(&mut job, &line[cursor..t.start], TEXT, &font);
                    }
                    let color = match t.kind {
                        TokenKind::Heading => ACCENT,
                        TokenKind::InlineCode => SYN_STR,
                        TokenKind::Link => SYN_LINK,
                        TokenKind::Keyword => SYN_KEYWORD,
                        TokenKind::Comment => SYN_COMMENT,
                        _ => TEXT,
                    };
                    push_run(&mut job, &line[t.start..t.end], color, &font);
                    cursor = t.end;
                }
                if cursor < line.len() {
                    push_run(&mut job, &line[cursor..], TEXT, &font);
                }
                if line.is_empty() {
                    push_run(&mut job, " ", TEXT, &font);
                }
                ui.label(job);
            }
        });
}

/// 编辑器 tab：行号 + 可选 blame 列 + 高亮只读视图 + 可编辑缓冲 + 保存。
///
/// 编辑用单独的 `TextEdit`，这样输入法、撤销、光标行为都交给 egui；
/// 上方高亮视图负责着色与标红，两者共用同一份 `text`。
pub fn editor_tab(ui: &mut egui::Ui, app: &mut App) {
    let Some(doc) = app.doc.clone() else {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("从「文件」tab 点一个文件，或用 README 的「用编辑器打开」")
                .color(DIM)
                .size(12.0),
        );
        return;
    };

    // ---- 工具条 ----
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(doc.path.clone())
                .monospace()
                .size(11.5)
                .color(TEXT),
        );
        ui.label(
            egui::RichText::new(doc.lang.label())
                .color(ACCENT)
                .size(10.0),
        );
        if doc.dirty() {
            ui.label(egui::RichText::new("● 未保存").color(WARN).size(10.5));
        } else {
            ui.label(egui::RichText::new("已保存").color(OK).size(10.5));
        }
        ui.label(
            egui::RichText::new(format!("{} 行", doc.text.lines().count()))
                .color(DIM)
                .size(10.0),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let save = ui.add_enabled(doc.dirty(), egui::Button::new("保存"));
            if save.clicked() {
                app.save_doc();
            }
            if ui
                .add_enabled(doc.dirty(), egui::Button::new("放弃修改"))
                .clicked()
            {
                app.revert_doc();
            }
            ui.checkbox(&mut app.show_blame, "blame");
        });
    });

    if let Some(err) = &doc.blame_error {
        ui.label(
            egui::RichText::new(format!("blame 不可用：{err}"))
                .color(DIM)
                .size(10.0),
        );
    }
    if !doc.diags.is_empty() {
        ui.label(
            egui::RichText::new(format!("{} 处语法提示", doc.diags.len()))
                .color(ERR)
                .size(10.5),
        );
    }
    ui.separator();

    let show_blame = app.show_blame;
    let mut edited = doc.text.clone();
    let bad_lines: Vec<usize> = doc.diags.iter().map(|d| d.line).collect();

    // 上：着色预览（高亮 + 标红）
    ui.label(egui::RichText::new("着色预览").color(DIM).size(10.0));
    let preview = doc.clone();
    egui::ScrollArea::both()
        .id_salt("editor_view")
        .auto_shrink([false, false])
        .max_height(240.0)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                gutter(ui, &preview, &bad_lines, show_blame);
                ui.separator();
                ui.vertical(|ui| {
                    let src_lines: Vec<&str> = preview.text.lines().collect();
                    for (i, line) in src_lines.iter().enumerate() {
                        ui.label(highlighted(line, preview.tokens.get(i).map(Vec::as_slice)));
                    }
                });
            });
        });

    ui.separator();
    ui.label(egui::RichText::new("编辑区").color(DIM).size(10.0));
    let line_count = edited.lines().count().max(12);
    egui::ScrollArea::both()
        .id_salt("editor_edit")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                gutter(ui, &doc, &bad_lines, show_blame);
                ui.separator();
                ui.add(
                    egui::TextEdit::multiline(&mut edited)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(line_count)
                        .frame(false),
                );
            });
        });

    // 诊断清单
    if !doc.diags.is_empty() {
        ui.separator();
        ui.label(egui::RichText::new("语法提示").color(ERR).size(10.5));
        egui::ScrollArea::vertical()
            .id_salt("diag_scroll")
            .max_height(90.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for d in &doc.diags {
                    ui.label(
                        egui::RichText::new(format!("{}:{}  {}", d.line, d.column, d.message))
                            .color(ERR)
                            .size(10.5)
                            .monospace(),
                    );
                }
            });
    }

    // 回写编辑结果
    if let Some(d) = &mut app.doc
        && d.text != edited
    {
        d.text = edited;
        d.reanalyze();
    }
}

/// 行号 + blame 列。
fn gutter(ui: &mut egui::Ui, doc: &OpenDoc, bad_lines: &[usize], show_blame: bool) {
    let line_count = doc.text.lines().count().max(1);
    ui.vertical(|ui| {
        for i in 0..line_count {
            let line_no = i + 1;
            let bad = bad_lines.contains(&line_no);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{line_no:>4}"))
                        .monospace()
                        .size(11.0)
                        .color(if bad { ERR } else { DIM }),
                );
                if show_blame {
                    let hit = doc.blame.iter().find(|b| b.line_no == line_no);
                    let (label, color) = match hit {
                        Some(b) => (format!("{} {}", b.commit, short_author(&b.author)), MUTED),
                        None => ("—".to_string(), DIM),
                    };
                    ui.label(
                        egui::RichText::new(label)
                            .monospace()
                            .size(10.0)
                            .color(color),
                    );
                }
            });
        }
    });
}

/// 把一行按 token 着色成 `LayoutJob`。
fn highlighted(
    line: &str,
    tokens: Option<&[git_switch::model::highlight::Token]>,
) -> egui::text::LayoutJob {
    let font = egui::FontId::monospace(11.5);
    let mut job = egui::text::LayoutJob::default();
    let mut cursor = 0usize;
    for t in tokens.unwrap_or(&[]) {
        if t.start >= line.len() || t.end > line.len() || t.start > t.end {
            break;
        }
        if t.start > cursor {
            push_run(&mut job, &line[cursor..t.start], TEXT, &font);
        }
        push_run(&mut job, &line[t.start..t.end], token_color(t.kind), &font);
        cursor = t.end;
    }
    if cursor < line.len() {
        push_run(&mut job, &line[cursor..], TEXT, &font);
    }
    if line.is_empty() {
        push_run(&mut job, " ", TEXT, &font);
    }
    job
}

/// 加密单行运行片段。
fn push_run(
    job: &mut egui::text::LayoutJob,
    text: &str,
    color: egui::Color32,
    font: &egui::FontId,
) {
    if text.is_empty() {
        return;
    }
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
}

/// token 类别到颜色。
fn token_color(kind: TokenKind) -> egui::Color32 {
    match kind {
        TokenKind::Keyword => SYN_KEYWORD,
        TokenKind::Str => SYN_STR,
        TokenKind::Comment => SYN_COMMENT,
        TokenKind::Number => SYN_NUMBER,
        TokenKind::Annotation | TokenKind::Tag | TokenKind::Attribute => SYN_ANNOTATION,
        TokenKind::Heading => ACCENT,
        TokenKind::InlineCode => SYN_STR,
        TokenKind::Link => SYN_LINK,
    }
}

/// blame 列里只留作者名的前 8 个字符，避免挤掉正文。
fn short_author(name: &str) -> String {
    let trimmed = name.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        trimmed.to_string()
    } else {
        format!("{}…", chars[..8].iter().collect::<String>())
    }
}

/// 克隆弹窗。
pub fn clone_modal(ctx: &egui::Context, app: &mut App) {
    if !app.clone_buf.open {
        return;
    }
    let mut open = app.clone_buf.open;
    let mut run = false;

    egui::Window::new("克隆仓库")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            egui::Grid::new("clone_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("仓库地址");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut app.clone_buf.url)
                            .hint_text("git@github.com:owner/repo.git")
                            .desired_width(360.0),
                    );
                    if resp.changed() {
                        app.refresh_clone_hint();
                    }
                    ui.end_row();

                    ui.label("克隆到");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.clone_buf.parent)
                            .hint_text("父目录，例如 /Users/you/WorkSpace")
                            .desired_width(360.0),
                    );
                    ui.end_row();

                    ui.label("目录名");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.clone_buf.dir_name)
                            .hint_text("留空则按仓库名推导")
                            .desired_width(360.0),
                    );
                    ui.end_row();

                    ui.label("平台");
                    ui.label(
                        egui::RichText::new(if app.clone_buf.platform_hint.is_empty() {
                            "（由地址自动推导）"
                        } else {
                            &app.clone_buf.platform_hint
                        })
                        .color(
                            if app.clone_buf.platform_hint.contains("未匹配")
                                || app.clone_buf.platform_hint.contains("无法解析")
                            {
                                WARN
                            } else {
                                ACCENT
                            },
                        )
                        .size(11.0),
                    );
                    ui.end_row();

                    ui.label("身份");
                    let names: Vec<(String, String)> = app
                        .cfg
                        .identities
                        .iter()
                        .map(|i| (i.id.clone(), i.label.clone()))
                        .collect();
                    let current = names
                        .iter()
                        .find(|(id, _)| *id == app.clone_buf.identity_id)
                        .map(|(_, l)| l.clone())
                        .unwrap_or_else(|| "不写入身份（跟随全局）".to_string());
                    egui::ComboBox::from_id_salt("clone_identity")
                        .selected_text(current)
                        .width(360.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut app.clone_buf.identity_id,
                                String::new(),
                                "不写入身份（跟随全局）",
                            );
                            for (id, label) in &names {
                                ui.selectable_value(
                                    &mut app.clone_buf.identity_id,
                                    id.clone(),
                                    label,
                                );
                            }
                        });
                    ui.end_row();
                });

            // 目标路径预览
            if let Ok(target) = git_switch::git::clone::CloneRequest::new(
                app.clone_buf.url.clone(),
                app.clone_buf.parent.clone(),
            )
            .with_dir_name(app.clone_buf.dir_name.clone())
            .target_path()
            {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("将克隆到 {}", target.display()))
                        .color(DIM)
                        .size(10.5)
                        .monospace(),
                );
            }

            if let Some(err) = &app.clone_buf.error {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(err).color(ERR).size(11.0));
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("开始克隆").clicked() {
                    run = true;
                }
                if ui.button("取消").clicked() {
                    app.clone_buf.open = false;
                }
                ui.label(
                    egui::RichText::new("克隆成功后会把选中的身份写进新仓库的 .git/config")
                        .color(DIM)
                        .size(10.0),
                );
            });
        });

    if run {
        app.run_clone();
    }
    if !open {
        app.clone_buf.open = false;
    }
}
