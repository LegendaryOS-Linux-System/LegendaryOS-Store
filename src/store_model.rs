// src/store_model.rs — Application state + Slint bridge
//
// StoreModel owns:
//   - Full catalog (all AppRecord entries)
//   - Current filter (search query + category)
//   - Slint weak handle for UI updates

use std::sync::{Arc, Mutex};

use slint::{Color, ModelRc, SharedString, VecModel, Weak};

use crate::app_data::{builtin_catalog, builtin_categories, AppRecord};
use crate::flatpak;

// Re-export generated Slint types
use crate::{AppEntry, CategoryEntry, MainWindow};

// ─── Internal state ───────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct State {
    all_apps: Vec<AppRecord>,
    installed_ids: Vec<String>,
    update_ids: Vec<String>,
    search: String,
    category: String,
}

impl State {
    fn filtered(&self) -> Vec<AppRecord> {
        let search_lower = self.search.to_lowercase();
        let cat = &self.category;

        self.all_apps
            .iter()
            .filter(|a| {
                // Category filter
                let cat_ok = cat.is_empty()
                    || cat == "all"
                    || cat == &a.category
                    || (cat == "installed" && self.installed_ids.contains(&a.flatpak_id))
                    || (cat == "updates" && self.update_ids.contains(&a.flatpak_id));

                // Search filter
                let search_ok = search_lower.is_empty()
                    || a.name.to_lowercase().contains(&search_lower)
                    || a.summary.to_lowercase().contains(&search_lower)
                    || a.developer.to_lowercase().contains(&search_lower)
                    || a.category.to_lowercase().contains(&search_lower);

                cat_ok && search_ok
            })
            .cloned()
            .collect()
    }
}

// ─── StoreModel ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StoreModel {
    state: Arc<Mutex<State>>,
    window: Weak<MainWindow>,
}

impl StoreModel {
    pub fn new(window: Weak<MainWindow>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                all_apps: builtin_catalog(),
                category: "all".to_string(),
                ..Default::default()
            })),
            window,
        }
    }

    /// Initial setup: push catalog + categories to UI, then check installed.
    pub fn init(&self) {
        self.push_categories();
        self.push_filtered_apps();

        // Async: refresh which apps are installed
        let m = self.clone();
        tokio::spawn(async move {
            m.refresh_installed().await;
        });
    }

    // ── UI push helpers ───────────────────────────────────────────────────────

    fn push_categories(&self) {
        let cats: Vec<CategoryEntry> = builtin_categories()
            .into_iter()
            .map(|(id, label, icon)| CategoryEntry {
                id: id.into(),
                label: label.into(),
                icon: icon.into(),
            })
            .collect();

        let model = ModelRc::new(VecModel::from(cats));
        self.window
            .upgrade_in_event_loop(move |w| {
                w.set_categories(model);
            })
            .ok();
    }

    fn push_filtered_apps(&self) {
        let state = self.state.lock().unwrap();
        let filtered = state.filtered();
        let installed = state.installed_ids.clone();
        let updates = state.update_ids.clone();
        drop(state);

        let entries: Vec<AppEntry> = filtered
            .into_iter()
            .map(|a| app_record_to_entry(&a, &installed, &updates))
            .collect();

        let count = entries.iter().filter(|e| e.is_installed).count() as i32;
        let upd_count = entries.iter().filter(|e| e.is_updating).count() as i32;
        let model = ModelRc::new(VecModel::from(entries));

        self.window
            .upgrade_in_event_loop(move |w| {
                w.set_apps(model);
                w.set_installed_count(count);
                w.set_updates_count(upd_count);
                w.set_loading(false);
            })
            .ok();
    }

    // ── Public actions ────────────────────────────────────────────────────────

    pub fn filter_search(&self, query: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.search = query.to_string();
        }
        self.push_filtered_apps();
    }

    pub fn filter_category(&self, cat: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.category = cat.to_string();
        }
        self.push_filtered_apps();
    }

    pub fn open_detail(&self, id: &str) {
        let state = self.state.lock().unwrap();
        let installed = state.installed_ids.clone();
        let updates = state.update_ids.clone();
        let found = state
            .all_apps
            .iter()
            .find(|a| a.id == id)
            .cloned();
        drop(state);

        if let Some(rec) = found {
            let entry = app_record_to_entry(&rec, &installed, &updates);
            self.window
                .upgrade_in_event_loop(move |w| {
                    w.set_selected_app(entry);
                    w.set_detail_visible(true);
                })
                .ok();
        }
    }

    pub async fn refresh_installed(&self) {
        // Show loading spinner
        self.window
            .upgrade_in_event_loop(|w| w.set_loading(true))
            .ok();

        let installed = flatpak::list_installed().await;
        let updates = flatpak::list_updates().await;

        {
            let mut s = self.state.lock().unwrap();
            s.installed_ids = installed;
            s.update_ids = updates;
        }

        self.push_filtered_apps();
    }

    pub async fn install(&self, flatpak_id: &str) {
        let app_name = {
            let s = self.state.lock().unwrap();
            s.all_apps
                .iter()
                .find(|a| a.flatpak_id == flatpak_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| flatpak_id.to_string())
        };

        // Show toast
        let name_clone: SharedString = app_name.clone().into();
        self.window
            .upgrade_in_event_loop(move |w| {
                w.set_toast_app(name_clone);
                w.set_toast_progress(0.0);
                w.set_toast_visible(true);
            })
            .ok();

        let window_weak = self.window.clone();
        let result = flatpak::install(flatpak_id, move |progress| {
            let w = window_weak.clone();
            // Slint upgrades must happen on the UI thread via event loop
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() {
                    win.set_toast_progress(progress);
                }
            });
        })
        .await;

        // Hide toast after short delay
        let w2 = self.window.clone();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        w2.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();

        if result.success {
            self.refresh_installed().await;
        } else {
            eprintln!("[store] install failed: {}", result.output);
        }
    }

    pub async fn remove(&self, flatpak_id: &str) {
        let result = flatpak::remove(flatpak_id).await;
        if result.success {
            self.refresh_installed().await;
            // Close detail panel if it was showing this app
            self.window
                .upgrade_in_event_loop(|w| w.set_detail_visible(false))
                .ok();
        } else {
            eprintln!("[store] remove failed: {}", result.output);
        }
    }
}

// ─── Conversion helper ────────────────────────────────────────────────────────

fn app_record_to_entry(
    a: &AppRecord,
    installed: &[String],
    updates: &[String],
) -> AppEntry {
    let is_installed = installed.contains(&a.flatpak_id);
    let is_updating = updates.contains(&a.flatpak_id);

    AppEntry {
        id: a.id.clone().into(),
        flatpak_id: a.flatpak_id.clone().into(),
        name: a.name.clone().into(),
        summary: a.summary.clone().into(),
        description: a.description.clone().into(),
        version: a.version.clone().into(),
        developer: a.developer.clone().into(),
        category: category_label(&a.category).into(),
        icon_letter: a.icon_letter.clone().into(),
        icon_color: hex_to_color(&a.icon_color_hex),
        rating: a.rating,
        download_count: a.download_count.clone().into(),
        size_mb: a.size_mb,
        is_installed,
        is_updating,
        tags: ModelRc::new(VecModel::from(vec![])),
    }
}

fn hex_to_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return Color::from_rgb_u8(147, 51, 234); // fallback purple
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(147);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(51);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(234);
    Color::from_rgb_u8(r, g, b)
}

fn category_label(id: &str) -> &str {
    match id {
        "internet"     => "Internet",
        "multimedia"   => "Multimedia",
        "graphics"     => "Graphics",
        "productivity" => "Office",
        "development"  => "Dev",
        "games"        => "Games",
        "tools"        => "Tools",
        _              => "Other",
    }
}
