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
            .with_title("note-sync"),
        ..Default::default()
    };

    eframe::run_native(
        "note-sync",
        options,
        Box::new(move |cc| {
            let icon = make_icon();
            let menu = Menu::new();
            let _show = MenuItem::new("Show Window", true, None);
            let _quit = MenuItem::new("Quit", true, None);
            let _ = menu.append(&_show);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&_quit);

            let tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("note-sync")
                .with_icon(icon)
                .build()
                .ok();
            *tray_c.borrow_mut() = tray;

            // Keep menu item IDs alive by leaking intentionally for MVP.
            // (MenuItems must outlive the tray menu on some platforms.)
            std::mem::forget(_show);
            std::mem::forget(_quit);

            let app = NoteSyncApp::new(
                config,
                dirty_editor,
                scheduler,
                tray_c.borrow_mut().take(),
            );
            // Prefer dark-ish for long reading; egui defaults are fine.
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(app))
        }),
    )
}

fn make_icon() -> Icon {
    // 32x32 solid glyph — avoids shipping a PNG for MVP.
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let i = ((y * size + x) * 4) as usize;
            let on = x > 6 && x < 26 && y > 6 && y < 26;
            if on {
                rgba[i] = 90;
                rgba[i + 1] = 180;
                rgba[i + 2] = 120;
                rgba[i + 3] = 255;
            }
        }
    }
    Icon::from_rgba(rgba, size, size).expect("icon")
}
