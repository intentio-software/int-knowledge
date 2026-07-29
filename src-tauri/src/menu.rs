//! The native application menu.
//!
//! Deliberately the same shape as Intentio Mind Map's: items carry no behaviour
//! of their own, they emit a `menu-action` event with their id and the Angular
//! side runs the handler the in-app controls already use. One implementation per
//! action, and two apps that feel like one suite.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Runtime};

/// Event the frontend listens on. The payload is the menu item id.
pub const MENU_EVENT: &str = "menu-action";

/// A recently opened vault, as shown under File → Open Recent.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RecentEntry {
    pub name: String,
}

pub fn build<R: Runtime>(app: &AppHandle<R>, recents: &[RecentEntry]) -> tauri::Result<Menu<R>> {
    // --- application menu (macOS only; ignored elsewhere) -------------------
    let app_menu = Submenu::with_items(
        app,
        "Intentio Knowledge",
        true,
        &[
            &MenuItem::with_id(app, "about", "About Intentio Knowledge", true, None::<&str>)?,
            &MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::show_all(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    // --- File ---------------------------------------------------------------
    let recent_items: Vec<MenuItem<R>> = recents
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            MenuItem::with_id(app, format!("recent:{index}"), &entry.name, true, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;

    let recent_menu = if recent_items.is_empty() {
        Submenu::with_items(
            app,
            "Open Recent",
            true,
            &[&MenuItem::with_id(app, "recent-empty", "No recent vaults", false, None::<&str>)?],
        )?
    } else {
        let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
            recent_items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<R>).collect();
        Submenu::with_items(app, "Open Recent", true, &refs)?
    };

    let file_menu = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, "new-note", "New Note", true, Some("CmdOrCtrl+N"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open-vault", "Open Vault…", true, Some("CmdOrCtrl+Shift+O"))?,
            &MenuItem::with_id(app, "new-vault", "New Vault…", true, None::<&str>)?,
            &recent_menu,
            &MenuItem::with_id(app, "close-vault", "Close Vault", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "save", "Save", true, Some("CmdOrCtrl+S"))?,
            &MenuItem::with_id(app, "rename", "Rename or Move Note…", true, None::<&str>)?,
            &MenuItem::with_id(app, "delete", "Delete Note…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    // --- Edit ---------------------------------------------------------------
    // Unlike Mind Map, these are the predefined items: the editor is a text
    // surface, so the webview's own undo and clipboard are exactly what is
    // wanted, and CodeMirror handles the keys when it has focus.
    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    // --- View ---------------------------------------------------------------
    let view_menu = Submenu::with_items(
        app,
        "View",
        true,
        &[
            &MenuItem::with_id(app, "toggle-view", "Toggle Read / Source", true, Some("CmdOrCtrl+E"))?,
            &MenuItem::with_id(app, "toggle-graph", "Graph View", true, Some("CmdOrCtrl+G"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "toggle-sidebar", "Toggle Sidebar", true, Some("CmdOrCtrl+B"))?,
            &MenuItem::with_id(app, "toggle-panel", "Toggle Side Panel", true, Some("CmdOrCtrl+\\"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "toggle-theme", "Switch Theme", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    // --- Go -----------------------------------------------------------------
    let go_menu = Submenu::with_items(
        app,
        "Go",
        true,
        &[
            &MenuItem::with_id(app, "jump", "Jump to Note…", true, Some("CmdOrCtrl+O"))?,
            &MenuItem::with_id(app, "search", "Search Notes…", true, Some("CmdOrCtrl+Shift+F"))?,
        ],
    )?;

    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?],
    )?;

    let help_menu = Submenu::with_items(
        app,
        "Help",
        true,
        &[
            &MenuItem::with_id(app, "about", "About Intentio Knowledge", true, None::<&str>)?,
            &MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "website", "Intentio Software", true, None::<&str>)?,
        ],
    )?;

    Menu::with_items(
        app,
        &[&app_menu, &file_menu, &edit_menu, &view_menu, &go_menu, &window_menu, &help_menu],
    )
}

/// Install the menu and forward every click to the frontend.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build(app, &[])?;
    app.set_menu(menu)?;

    app.on_menu_event(|app, event| {
        let id = event.id().as_ref().to_string();
        let _ = app.emit(MENU_EVENT, id);
    });

    Ok(())
}

/// Rebuild the menu with a new Open Recent list.
#[tauri::command]
pub fn set_recent_vaults<R: Runtime>(
    app: AppHandle<R>,
    recents: Vec<RecentEntry>,
) -> Result<(), String> {
    let menu = build(&app, &recents).map_err(|err| err.to_string())?;
    app.set_menu(menu).map_err(|err| err.to_string())?;
    Ok(())
}
