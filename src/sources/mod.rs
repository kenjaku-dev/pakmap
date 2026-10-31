//! Flatpak source surface for the rest of the crate (free functions only).
//!
//! `service-decomposition`: modular monolith — one binary, enforced module
//! boundaries. The search/install/update flows live in `src/search/` and
//! `src/install/`; this module exposes the flatpak primitives they call.
//! A generic `SourceBackend` trait existed until v0.9.4: one adapter
//! (`FlatpakBackend`), no second consumer — the deletion test called it a
//! hypothetical seam, so it is gone. Only add a seam when a second source
//! actually varies across it.

pub mod flatpak;

pub use flatpak::{
    check_flatpak_updates_cached, check_flatpak_updates_cached_for, check_flatpak_updates_for,
    check_flatpak_updates_with_config, clean_unused_flatpak, has_flatpak, install_flatpak_package,
    invalidate_flatpak_list_memo, invalidate_flatpak_memo, is_flatpak_installed, list_installed,
    list_installed_cached, remove_flatpak_package, search_flatpak_with_config, update_all_flatpak,
    update_flatpak_package, validate_app_id,
};
