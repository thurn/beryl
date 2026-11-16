//! macOS gives menu accelerators first refusal on key presses. While a manual table is
//! open, leave its shortcuts to the web view and route Quit to the table for confirmation.

use beryl_session::ipc::TableKeysRequest;

#[tauri::command]
#[cfg_attr(not(target_os = "macos"), allow(clippy::unnecessary_wraps))]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
pub fn table_keys(
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] app: tauri::AppHandle,
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] request: TableKeysRequest,
) -> Result<(), beryl_session::SessionError> {
    #[cfg(target_os = "macos")]
    mac::set(&app, request.on).map_err(|error| beryl_session::SessionError::Io {
        reason: error.to_string(),
    })?;
    Ok(())
}

/// Both the menu request and the web-view shortcut use the same confirmation.
#[cfg(desktop)]
#[tauri::command]
pub async fn table_quit(app: tauri::AppHandle) -> Result<(), beryl_session::SessionError> {
    use beryl_table_session::connections::TableConnections;
    use tauri::Manager;
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

    tauri::async_runtime::spawn_blocking(move || {
        if app
            .dialog()
            .message("Quit Beryl?")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Quit".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            app.state::<TableConnections>().shutdown();
            app.exit(0);
        }
    })
    .await
    .map_err(|error| beryl_session::SessionError::Io {
        reason: error.to_string(),
    })
}

#[cfg(mobile)]
#[tauri::command]
#[allow(clippy::unnecessary_wraps)]
pub fn table_quit() -> Result<(), beryl_session::SessionError> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub mod mac {
    use tauri::menu::{
        AboutMetadata, HELP_SUBMENU_ID, Menu, MenuBuilder, MenuEvent, MenuItemBuilder,
        SubmenuBuilder, WINDOW_SUBMENU_ID,
    };
    use tauri::{AppHandle, Emitter, Manager, Wry};

    const ASK_TO_QUIT: &str = "table-quit";

    #[derive(Debug, PartialEq, Eq)]
    enum Quit {
        Native,
        Ask { accelerator: &'static str },
    }

    #[derive(Debug, PartialEq, Eq)]
    struct MenuDescription {
        close_and_minimize: bool,
        quit: Quit,
    }

    fn describe(on: bool) -> MenuDescription {
        MenuDescription {
            close_and_minimize: !on,
            quit: if on {
                Quit::Ask {
                    accelerator: "Cmd+Q",
                }
            } else {
                Quit::Native
            },
        }
    }

    // Use the same item order and metadata as Tauri's default macOS menu.
    fn build(app: &AppHandle, description: &MenuDescription) -> tauri::Result<Menu<Wry>> {
        let package = app.package_info();
        let about = AboutMetadata {
            name: Some(package.name.clone()),
            version: Some(package.version.to_string()),
            copyright: app.config().bundle.copyright.clone(),
            authors: app.config().bundle.publisher.clone().map(|name| vec![name]),
            ..AboutMetadata::default()
        };
        let application = SubmenuBuilder::new(app, &package.name)
            .about(Some(about))
            .separator()
            .services()
            .separator()
            .hide()
            .hide_others()
            .separator();
        let application = match description.quit {
            Quit::Native => application.quit(),
            Quit::Ask { accelerator } => application.item(
                &MenuItemBuilder::with_id(ASK_TO_QUIT, format!("Quit {}", package.name))
                    .accelerator(accelerator)
                    .build(app)?,
            ),
        }
        .build()?;
        let mut menu = MenuBuilder::new(app).item(&application);
        let window = SubmenuBuilder::with_id(app, WINDOW_SUBMENU_ID, "Window");
        let window = if description.close_and_minimize {
            menu = menu.item(&SubmenuBuilder::new(app, "File").close_window().build()?);
            window.minimize().maximize().separator().close_window()
        } else {
            window.maximize()
        };
        menu.items(&[
            &SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?,
            &SubmenuBuilder::new(app, "View").fullscreen().build()?,
            &window.build()?,
            &SubmenuBuilder::with_id(app, HELP_SUBMENU_ID, "Help").build()?,
        ])
        .build()
    }

    struct Menus {
        standard: Menu<Wry>,
        table: Menu<Wry>,
    }

    pub fn setup(app: &AppHandle) -> tauri::Result<()> {
        let standard = app
            .menu()
            .map_or_else(|| build(app, &describe(false)), Ok)?;
        app.manage(Menus {
            standard,
            table: build(app, &describe(true))?,
        });
        Ok(())
    }

    pub fn set(app: &AppHandle, on: bool) -> tauri::Result<()> {
        let menus = app.state::<Menus>();
        let next = if on { &menus.table } else { &menus.standard };
        if app.menu().is_none_or(|current| current.id() != next.id()) {
            app.set_menu(next.clone())?;
        }
        Ok(())
    }

    #[allow(clippy::needless_pass_by_value)] // Tauri passes menu events by value.
    pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
        if event.id() == ASK_TO_QUIT
            && let Err(error) = app.emit(beryl_session::ipc::TABLE_QUIT_ASKED, ())
        {
            beryl_session::journal::warn("app", error.to_string(), serde_json::Value::Null);
        }
    }

    #[cfg(test)]
    #[test]
    fn table_menu_releases_window_shortcuts_and_intercepts_quit() {
        assert_eq!(
            describe(false),
            MenuDescription {
                close_and_minimize: true,
                quit: Quit::Native,
            }
        );
        assert_eq!(
            describe(true),
            MenuDescription {
                close_and_minimize: false,
                quit: Quit::Ask {
                    accelerator: "Cmd+Q",
                },
            }
        );
    }
}
