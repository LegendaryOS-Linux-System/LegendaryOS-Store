use serde::{Deserialize, Serialize};

/// Which delivery mechanism this package uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AppBackend {
    /// User-space sandboxed app from Flathub
    #[default]
    Flatpak,
    /// System-level package applied via bootc / dnf5
    Bootc,
}

/// Raw application record — shared between catalog.rs and store_model.rs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppRecord {
    /// Unique store ID (no dots, lowercase)
    pub id: String,
    /// Flatpak: reverse-DNS app ID  |  Bootc: RPM package name
    pub flatpak_id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub version: String,
    pub developer: String,
    /// Internal category key: internet / multimedia / graphics /
    ///   productivity / development / games / tools / system
    pub category: String,
    pub icon_letter: String,
    pub icon_color_hex: String,
    pub rating: f32,
    pub download_count: String,
    pub size_mb: f32,
    pub backend: AppBackend,
}
