#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(any(not(feature = "role-ban-only"), test))]
mod alien_preset;
mod alien_selection;
mod bank;
mod edit_policy;
mod model;
mod paths;
mod replay;
mod scan;
mod theme;
mod ui;

use eframe::egui;

#[cfg(not(feature = "role-ban-only"))]
const TITLE: &str = "缄默之陨 · 数据工作台 · 完整版";
#[cfg(feature = "role-ban-only")]
const TITLE: &str = "缄默之陨 · 数据工作台 · 普通版";

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 900.0])
            .with_min_inner_size([1000.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(|creation| {
            theme::configure(&creation.egui_ctx);
            Ok(Box::new(ui::Viewer::new(&creation.egui_ctx)))
        }),
    )
}
