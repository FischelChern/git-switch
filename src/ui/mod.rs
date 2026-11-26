//! 界面层：只负责渲染与把事件转成 [`crate::app::App`] 的方法调用。
//!
//! 依赖方向：`ui → app → { git, store, model }`。本层**不执行 git 命令**，
//! 也不写文件。

pub mod fonts;
pub mod panels;
pub mod theme;

use eframe::egui;

use crate::app::App;
use theme::*;

/// 每帧绘制整个窗口。
pub fn draw(ctx: &egui::Context, app: &mut App) {
    top_bar(ctx, app);
    panels::terminal(ctx, app);
    panels::sidebar(ctx, app);
    panels::projects(ctx, app);
    panels::detail(ctx, app);
    panels::editor::clone_modal(ctx, app);
    toast(ctx, app);
}

fn top_bar(ctx: &egui::Context, app: &mut App) {
    egui::TopBottomPanel::top("topbar")
        .exact_height(44.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new("GitSwitch").strong().size(14.0));
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{} 平台 · {} 身份 · {} 项目",
                        app.cfg.platforms.len(),
                        app.cfg.identities.len(),
                        app.cfg.repositories.len()
                    ))
                    .color(DIM)
                    .size(11.0),
                );
                if !app.scanned.is_empty() {
                    ui.label(
                        egui::RichText::new(format!("上次扫描发现 {} 个仓库", app.scanned.len()))
                            .color(theme::WARN)
                            .size(10.5),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("设置").clicked() {
                        app.push_log(crate::app::LogKind::Out, "设置面板尚未实现");
                        app.flash("设置尚未实现");
                    }
                    ui.add_space(4.0);
                    if ui.button("克隆").clicked() {
                        app.open_clone();
                    }
                    ui.add_space(4.0);
                    if ui.button("扫描").clicked() {
                        app.op_scan();
                    }
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.scan_dir)
                            .hint_text("要扫描的目录")
                            .desired_width(240.0),
                    );
                });
            });
        });
}

/// 右上角提示条。
fn toast(ctx: &egui::Context, app: &App) {
    let Some((text, left)) = &app.toast else {
        return;
    };
    let alpha = (left / 2.5).clamp(0.15, 1.0);
    egui::Area::new(egui::Id::new("toast"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 54.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(PANEL_2)
                .stroke(egui::Stroke::new(1.0_f32, LINE))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(text)
                            .color(OK.gamma_multiply(alpha))
                            .size(12.0),
                    );
                });
        });
}
