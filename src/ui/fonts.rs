//! 中文字体加载。
//!
//! egui 默认字体不含 CJK，不加载会显示成方块。三平台各试若干常见路径，
//! 都找不到时保持默认字体（英文可用，中文显示为方块）并把结果告诉调用方。

use std::sync::Arc;

use eframe::egui;

/// 按平台列出候选字体路径。
fn candidates() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut paths: Vec<&'static str> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push("/System/Library/Fonts/Supplemental/Arial Unicode.ttf");
        paths.push("/System/Library/Fonts/Hiragino Sans GB.ttc");
        paths.push("/Library/Fonts/Arial Unicode.ttf");
        paths.push("/System/Library/Fonts/STHeiti Light.ttc");
    }

    #[cfg(target_os = "windows")]
    {
        paths.push("C:/Windows/Fonts/msyh.ttc");
        paths.push("C:/Windows/Fonts/msyh.ttf");
        paths.push("C:/Windows/Fonts/simhei.ttf");
        paths.push("C:/Windows/Fonts/simsun.ttc");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        paths.push("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc");
        paths.push("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf");
        paths.push("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc");
        paths.push("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc");
        paths.push("/usr/share/fonts/truetype/arphic/uming.ttc");
    }

    paths
}

/// 安装中文字体，返回实际使用的路径。
///
/// 返回 `None` 表示没找到可用字体，界面中文会显示为方块。
pub fn install(ctx: &egui::Context) -> Option<String> {
    for path in candidates() {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("cjk".to_owned());
        }
        ctx.set_fonts(fonts);
        return Some(path.to_string());
    }
    None
}
