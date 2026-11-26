//! 配色与字体样式，集中管理视觉常量。
//!
//! 颜色值取自 `demo/index.html`，改界面时**两边一起改**。

use eframe::egui::{self, Color32};

/// 窗口背景。
pub const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1a);
/// 面板背景。
pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x24);
/// 次级面板背景。
pub const PANEL_2: Color32 = Color32::from_rgb(0x21, 0x25, 0x2d);
/// 分隔线。
pub const LINE: Color32 = Color32::from_rgb(0x2b, 0x30, 0x38);
/// 正文。
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xec);
/// 次要文字。
pub const MUTED: Color32 = Color32::from_rgb(0x8b, 0x93, 0xa1);
/// 更淡的文字。
pub const DIM: Color32 = Color32::from_rgb(0x65, 0x6d, 0x7b);
/// 主强调色。
pub const ACCENT: Color32 = Color32::from_rgb(0x5b, 0x9d, 0xff);
/// 成功。
pub const OK: Color32 = Color32::from_rgb(0x46, 0xc8, 0x8b);
/// 警告。
pub const WARN: Color32 = Color32::from_rgb(0xe0, 0xa3, 0x3e);
/// 错误。
pub const ERR: Color32 = Color32::from_rgb(0xff, 0x5c, 0x74);

/// 应用深色主题。
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(0x12, 0x15, 0x1a);
    visuals.faint_bg_color = PANEL_2;
    visuals.widgets.noninteractive.bg_stroke.color = LINE;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.35);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}
