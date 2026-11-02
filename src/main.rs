//! DC Config Explorer: a desktop explorer for the Dragon City game configuration.
//!
//! Fan-made, not affiliated with SocialPoint.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod explorer;
mod localization_view;
mod mapping;
mod settings;
mod stats_view;
mod worker;

use fastframe_fonts::FontSetup;

fn main() -> eframe::Result<()> {
    let found = settings::DIRS.discover(|_, _| Ok(()));
    let dirs = found.dirs;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DC Config Explorer")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "DC Config Explorer",
        options,
        Box::new(move |cc| {
            // Settings are written by a background writer; plain writes from
            // this thread would stall the window.
            fastframe_file::mark_ui_thread();
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app::ExplorerApp::new(cc, dirs)))
        }),
    )
}

/// Inter at every weight plus system fallbacks, rendered like the desktop does.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontSetup::default().definitions();
    let rendering = fastframe_text::detect();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        rendering.apply_to_visuals(&mut style.visuals);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}
