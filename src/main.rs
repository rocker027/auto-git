#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use note_sync::app_ui::NoteSyncApp;
use note_sync::config::AppConfig;
use note_sync::scheduler::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

fn main() -> eframe::Result {
    let config = Arc::new(Mutex::new(AppConfig::load().unwrap_or_default()));
    let dirty_editor = Arc::new(AtomicBool::new(false));
    let scheduler = Scheduler::start(config.clone(), dirty_editor.clone());

    let tray_holder: Rc<RefCell<Option<tray_icon::TrayIcon>>> = Rc::new(RefCell::new(None));
    let tray_c = tray_holder.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Note Sync"),
        ..Default::default()
    };

    eframe::run_native(
        "note-sync",
        options,
        Box::new(move |cc| {
            let icon = make_icon();
            let menu = Menu::new();
            let show_item = MenuItem::new("顯示視窗", true, None);
            let quit_item = MenuItem::new("結束", true, None);
            let _ = menu.append(&show_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&quit_item);

            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Note Sync")
                .with_icon(icon)
                .with_icon_as_template(true)
                .build()
                .ok();
            *tray_c.borrow_mut() = tray;

            install_cjk_fallback(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let app = NoteSyncApp::new(
                config,
                dirty_editor,
                scheduler,
                tray_c.borrow_mut().take(),
                show_item,
                quit_item,
            );
            Ok(Box::new(app))
        }),
    )
}

fn install_cjk_fallback(ctx: &egui::Context) {
    let Some(bytes) = note_sync::app_ui::cjk_font_bytes() else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.push("cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn make_icon() -> Icon {
    let size = 32u32;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for row in 0..size {
        for column in 0..size {
            let edge = column == 9 || column == 22 || row == 7 || row == 25;
            let inside = (9..=22).contains(&column) && (7..=25).contains(&row);
            if inside && edge {
                let index = ((row * size + column) * 4) as usize;
                pixels[index + 3] = 255;
            }
        }
    }
    Icon::from_rgba(pixels, size, size).expect("icon")
}
