//! Closing the window keeps the app in the menu bar; only Quit ends it.

use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tray_icon::menu::{MenuEvent, MenuId};

/// What to do when the window is asked to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Let the window close and the app end.
    Allow,
    /// Keep running; hide the window into the menu bar.
    Hide,
    /// Text could not be saved: ask first, then hide.
    AskThenHide,
    /// Text could not be saved: ask first, then quit.
    AskThenQuit,
}

/// `is_quitting`: the request came from ⌘Q or the menu's 結束, not the window's close button.
/// `may_close`: the save question was already answered for this quit.
/// `save` runs only when the answer depends on it.
pub fn on_close_request(is_quitting: bool, may_close: bool, save: impl FnOnce() -> bool) -> CloseAction {
    if may_close {
        return CloseAction::Allow;
    }
    match (is_quitting, save()) {
        (false, true) => CloseAction::Hide,
        (false, false) => CloseAction::AskThenHide,
        (true, true) => CloseAction::Allow,
        (true, false) => CloseAction::AskThenQuit,
    }
}

/// Handles the tray and app-menu items as AppKit delivers them (on the main thread),
/// instead of polling a channel from `update`.
pub fn install_menu_handler(
    ctx: egui::Context,
    show_id: MenuId,
    quit_ids: Vec<MenuId>,
    quitting: Arc<AtomicBool>,
) {
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == show_id {
            show_app(&ctx);
        } else if quit_ids.contains(&event.id) {
            // The close request is handled in `update`; the window only comes back
            // if a "could not save" question has to be asked.
            quitting.store(true, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }));
    // Pointer events over the tray icon are not used; drop them instead of queueing forever.
    tray_icon::TrayIconEvent::set_event_handler(Some(|_| {}));
}

/// Hides the whole app like ⌘H: the window goes away, the menu bar icon and backups stay.
pub fn hide_app(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).hide(None);
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
}

pub fn show_app(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.unhide(None);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_button_hides_after_saving() {
        assert_eq!(on_close_request(false, false, || true), CloseAction::Hide);
        assert_eq!(on_close_request(false, false, || false), CloseAction::AskThenHide);
    }

    #[test]
    fn quit_ends_the_app_only_when_nothing_is_left_unsaved() {
        assert_eq!(on_close_request(true, false, || true), CloseAction::Allow);
        assert_eq!(on_close_request(true, false, || false), CloseAction::AskThenQuit);
    }

    #[test]
    fn an_answered_question_lets_the_close_through_without_saving_again() {
        let mut saved = false;
        assert_eq!(on_close_request(false, true, || { saved = true; true }), CloseAction::Allow);
        assert!(!saved);
    }
}
