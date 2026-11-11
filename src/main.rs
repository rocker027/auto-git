#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use note_sync::app_ui::{NoteSyncApp, SystemMenus};
use note_sync::config::AppConfig;
use note_sync::scheduler::Scheduler;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};

fn main() -> eframe::Result {
    let config = Arc::new(Mutex::new(AppConfig::load().unwrap_or_default()));
    let dirty_editor = Arc::new(AtomicBool::new(false));
    let scheduler = Scheduler::start(config.clone(), dirty_editor.clone());

    let tray_holder: Rc<RefCell<Option<tray_icon::TrayIcon>>> = Rc::new(RefCell::new(None));
    let tray_c = tray_holder.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([640.0, 420.0])
            .with_title("Note Sync"),
        ..Default::default()
    };

    eframe::run_native(
        "note-sync",
        options,
        Box::new(move |cc| {
            let icon = make_icon();
            let menu = Menu::new();
            let backup_status = note_sync::tray_status::TrayStatus::default();
            let show_item = MenuItem::new("顯示視窗", true, None);
            let quit_item = MenuItem::new("結束", true, None);
            let _ = menu.append(&backup_status.item);
            let _ = menu.append(&PredefinedMenuItem::separator());
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
            note_sync::widgets::apply_style(&cc.egui_ctx);
            let (app_menu, app_quit) = make_app_menu();
            let quitting = Arc::new(AtomicBool::new(false));
            note_sync::window_life::install_menu_handler(
                cc.egui_ctx.clone(),
                show_item.id().clone(),
                vec![quit_item.id().clone(), app_quit.id().clone()],
                quitting.clone(),
            );
            let menus = SystemMenus {
                tray: tray_c.borrow_mut().take(),
                show_item,
                backup_status,
                quit_items: vec![quit_item, app_quit],
                app_menu,
                quitting,
            };
            let app = NoteSyncApp::new(config, dirty_editor, scheduler, menus);
            Ok(Box::new(app))
        }),
    )
}

/// Replaces winit's default app menu, whose Quit ends the app without asking
/// about unsaved text. Our ⌘Q item is handled like the tray "結束".
fn make_app_menu() -> (Option<Menu>, MenuItem) {
    let quit = MenuItem::new(
        "結束 Note Sync",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
    );
    if !cfg!(target_os = "macos") {
        return (None, quit);
    }
    let app = Submenu::with_items(
        "Note Sync",
        true,
        &[
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ],
    );
    let menu = Menu::new();
    match app.and_then(|app| menu.append(&app)) {
        Ok(()) => {
            #[cfg(target_os = "macos")]
            menu.init_for_nsapp();
            (Some(menu), quit)
        }
        Err(err) => {
            eprintln!("note-sync: app menu not installed, ⌘Q will not ask about unsaved text: {err}");
            (None, quit)
        }
    }
}

fn install_cjk_fallback(ctx: &egui::Context) {
    let Some(bytes) = note_sync::platform::shared_cjk_font() else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.push("cjk".to_owned());
        }
    }
    // Use locally installed macOS fonts; no system font is bundled or redistributed.
    for (name, bytes, family) in [
        ("macos-ui", note_sync::platform::shared_system_ui_font(), egui::FontFamily::Proportional),
        ("macos-mono", note_sync::platform::shared_system_mono_font(), egui::FontFamily::Monospace),
    ] {
        if let Some(bytes) = bytes {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_static(bytes)));
            fonts.families.entry(family).or_default().insert(0, name.into());
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
