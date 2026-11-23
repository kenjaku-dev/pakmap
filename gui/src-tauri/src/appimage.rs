//! Tauri commands for the AppImage surface (v0.12.0).
//!
//! Every command is async and moves the blocking filesystem/network work to
//! `spawn_blocking`, so the webview main thread stays responsive. Commands are
//! crate-private by the repo's rustc-1.99 / Tauri-macro convention.

use std::path::Path;

use pakmap::appimage::Entry;

use crate::{GuiError, gui_error};

#[tauri::command]
pub(crate) async fn appimage_list() -> Result<Vec<Entry>, GuiError> {
    let handle = tauri::async_runtime::spawn_blocking(pakmap::appimage::service::list);
    match handle.await {
        Ok(Ok(entries)) => Ok(entries),
        Ok(Err(e)) => Err(gui_error("appimage_list_failed", e, None)),
        Err(e) => Err(gui_error(
            "appimage_worker_failed",
            format!("AppImage worker failed: {e}"),
            None,
        )),
    }
}

#[tauri::command]
pub(crate) async fn appimage_integrate(
    path: String,
    name: Option<String>,
) -> Result<Entry, GuiError> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        pakmap::appimage::service::integrate(Path::new(&path), name.as_deref())
    });
    match handle.await {
        Ok(Ok(entry)) => Ok(entry),
        Ok(Err(e)) => Err(gui_error("appimage_integrate_failed", e, None)),
        Err(e) => Err(gui_error(
            "appimage_worker_failed",
            format!("AppImage worker failed: {e}"),
            None,
        )),
    }
}

#[tauri::command]
pub(crate) async fn appimage_install(
    url: String,
    sha256: Option<String>,
    allow_unverified: Option<bool>,
    name: Option<String>,
) -> Result<Entry, GuiError> {
    let handle = tauri::async_runtime::spawn_blocking(move || {
        pakmap::appimage::service::install(
            &url,
            sha256.as_deref(),
            allow_unverified.unwrap_or(false),
            name.as_deref(),
        )
        .map(|installed| installed.entry)
    });
    match handle.await {
        Ok(Ok(entry)) => Ok(entry),
        Ok(Err(e)) => Err(gui_error("appimage_install_failed", e, None)),
        Err(e) => Err(gui_error(
            "appimage_worker_failed",
            format!("AppImage worker failed: {e}"),
            None,
        )),
    }
}

#[tauri::command]
pub(crate) async fn appimage_remove(id: String) -> Result<(), GuiError> {
    let handle =
        tauri::async_runtime::spawn_blocking(move || pakmap::appimage::service::remove(&id));
    match handle.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(gui_error("appimage_remove_failed", e, None)),
        Err(e) => Err(gui_error(
            "appimage_worker_failed",
            format!("AppImage worker failed: {e}"),
            None,
        )),
    }
}
