use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum AppBackend {
    #[default]
    Flatpak,
    Bootc,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppRecord {
    pub id:             String,
    pub flatpak_id:     String,
    pub name:           String,
    pub summary:        String,
    pub description:    String,
    pub version:        String,
    pub developer:      String,
    pub category:       String,
    pub icon_letter:    String,
    pub icon_color_hex: String,
    pub icon_url:       String,   // remote icon URL (future: cache to disk)
    pub rating:         f32,
    pub download_count: String,
    pub size_mb:        f32,
    pub backend:        AppBackend,
}
