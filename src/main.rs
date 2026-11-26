//! GitSwitch 程序入口。
//!
//! 只做三件事：初始化日志、创建窗口、把控制权交给 [`app::App`]。

mod app;
mod ui;

use eframe::egui;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_title("GitSwitch"),
        ..Default::default()
    };

    eframe::run_native(
        "GitSwitch",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
