use std::sync::{Arc, Mutex};
use slint::{Color, ModelRc, SharedString, VecModel, Weak};
use crate::app_data::{AppBackend, AppRecord};
use crate::{flatpak, bootc};
use crate::{AppEntry, CategoryEntry, MainWindow};

// ─── Sendable plain-data structs (no Rc inside) ───────────────────────────────

/// A version of AppEntry that is plain-data and Send.
#[derive(Clone)]
struct AppData {
    id: SharedString,
    flatpak_id: SharedString,
    name: SharedString,
    summary: SharedString,
    description: SharedString,
    version: SharedString,
    developer: SharedString,
    category: SharedString,
    icon_letter: SharedString,
    icon_color: Color,
    rating: f32,
    download_count: SharedString,
    size_mb: f32,
    is_installed: bool,
    is_updating: bool,
    backend: SharedString,
}

impl AppData {
    fn into_entry(self) -> AppEntry {
        AppEntry {
            id: self.id,
            flatpak_id: self.flatpak_id,
            name: self.name,
            summary: self.summary,
            description: self.description,
            version: self.version,
            developer: self.developer,
            category: self.category,
            icon_letter: self.icon_letter,
            icon_color: self.icon_color,
            rating: self.rating,
            download_count: self.download_count,
            size_mb: self.size_mb,
            is_installed: self.is_installed,
            is_updating: self.is_updating,
            backend: self.backend,
            // ModelRc constructed here on the UI thread — safe!
            tags: ModelRc::new(VecModel::from(vec![])),
        }
    }
}

// ─── Internal state ───────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct State {
    all_apps: Vec<AppRecord>,
    installed_ids: Vec<String>,   // flatpak app IDs
    bootc_layered: Vec<String>,   // bootc package names
    update_ids: Vec<String>,
    search: String,
    category: String,
}

impl State {
    fn filtered(&self) -> Vec<AppRecord> {
        let q = self.search.to_lowercase();
        let cat = &self.category;

        self.all_apps.iter().filter(|a| {
            let is_installed = match a.backend {
                AppBackend::Flatpak => self.installed_ids.contains(&a.flatpak_id),
                AppBackend::Bootc   => self.bootc_layered.contains(&a.flatpak_id),
            };
            let cat_ok = cat.is_empty() || cat == "all"
                || cat == &a.category
                || (cat == "installed" && is_installed)
                || (cat == "updates"   && self.update_ids.contains(&a.flatpak_id));
            let search_ok = q.is_empty()
                || a.name.to_lowercase().contains(&q)
                || a.summary.to_lowercase().contains(&q)
                || a.developer.to_lowercase().contains(&q)
                || a.category.to_lowercase().contains(&q);
            cat_ok && search_ok
        }).cloned().collect()
    }

    fn to_app_data(&self, records: Vec<AppRecord>) -> Vec<AppData> {
        records.into_iter().map(|a| {
            let is_installed = match a.backend {
                AppBackend::Flatpak => self.installed_ids.contains(&a.flatpak_id),
                AppBackend::Bootc   => self.bootc_layered.contains(&a.flatpak_id),
            };
            let is_updating = self.update_ids.contains(&a.flatpak_id);
            AppData {
                id:             a.id.into(),
                flatpak_id:     a.flatpak_id.into(),
                name:           a.name.into(),
                summary:        a.summary.into(),
                description:    a.description.into(),
                version:        a.version.into(),
                developer:      a.developer.into(),
                category:       category_label(&a.category).into(),
                icon_letter:    a.icon_letter.into(),
                icon_color:     hex_to_color(&a.icon_color_hex),
                rating:         a.rating,
                download_count: a.download_count.into(),
                size_mb:        a.size_mb,
                is_installed,
                is_updating,
                backend: match a.backend {
                    AppBackend::Flatpak => "flatpak".into(),
                    AppBackend::Bootc   => "bootc".into(),
                },
            }
        }).collect()
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
                category: "all".into(),
                ..Default::default()
            })),
            window,
        }
    }

    pub fn init(&self) {
        // Push empty state immediately so UI isn't blank
        self.push_ui(vec![], vec![], true);

        let m = self.clone();
        tokio::spawn(async move {
            // 1. Load catalog from Flathub API + bootc
            m.load_catalog().await;
            // 2. Check what's already installed
            m.refresh_installed().await;
        });
    }

    // ── Catalog loading ───────────────────────────────────────────────────────

    async fn load_catalog(&self) {
        let (flatpak_apps, bootc_apps) = tokio::join!(
            crate::catalog::fetch_flatpak_apps(),
            crate::catalog::fetch_bootc_packages(),
        );

        let mut all = flatpak_apps;
        all.extend(bootc_apps);

        {
            let mut s = self.state.lock().unwrap();
            s.all_apps = all;
        }
    }

    // ── Installed refresh ─────────────────────────────────────────────────────

    pub async fn refresh_installed(&self) {
        self.set_loading(true);

        let (installed, updates, layered) = tokio::join!(
            flatpak::list_installed(),
            flatpak::list_updates(),
            bootc::list_packages(),
        );

        let (filtered, installed_count, upd_count, cats) = {
            let mut s = self.state.lock().unwrap();
            s.installed_ids = installed;
            s.update_ids    = updates;
            s.bootc_layered = layered;
            let filtered = s.filtered();
            let data = s.to_app_data(filtered);
            let ic = data.iter().filter(|a| a.is_installed).count() as i32;
            let uc = s.update_ids.len() as i32;
            let cats = crate::catalog::categories();
            (data, ic, uc, cats)
        };

        self.push_ui(filtered, cats, false);
        self.push_counts(installed_count, upd_count);
    }

    // ── Filter helpers ────────────────────────────────────────────────────────

    pub fn filter_search(&self, query: &str) {
        { self.state.lock().unwrap().search = query.to_string(); }
        self.push_filtered();
    }

    pub fn filter_category(&self, cat: &str) {
        { self.state.lock().unwrap().category = cat.to_string(); }
        self.push_filtered();
    }

    fn push_filtered(&self) {
        let (data, cats) = {
            let s = self.state.lock().unwrap();
            let filtered = s.filtered();
            let data = s.to_app_data(filtered);
            let cats = crate::catalog::categories();
            (data, cats)
        };
        self.push_ui(data, cats, false);
    }

    pub fn open_detail(&self, id: &str) {
        let data = {
            let s = self.state.lock().unwrap();
            let rec = s.all_apps.iter().find(|a| a.id == id).cloned();
            rec.map(|r| {
                let v = s.to_app_data(vec![r]);
                v.into_iter().next()
            }).flatten()
        };
        if let Some(d) = data {
            self.window.upgrade_in_event_loop(move |w| {
                w.set_selected_app(d.into_entry());
                w.set_detail_visible(true);
            }).ok();
        }
    }

    // ── Install / Remove ──────────────────────────────────────────────────────

    pub async fn install(&self, flatpak_id: &str) {
        let (app_name, backend) = {
            let s = self.state.lock().unwrap();
            match s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id) {
                Some(r) => (r.name.clone(), r.backend.clone()),
                None    => (flatpak_id.to_string(), AppBackend::Flatpak),
            }
        };

        self.show_toast(app_name.clone().into(), 0.0);

        let ww = self.window.clone();
        let progress = move |p: f32| {
            let w = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() { win.set_toast_progress(p); }
            });
        };

        let success = match backend {
            AppBackend::Flatpak => {
                flatpak::install(flatpak_id, progress).await.success
            }
            AppBackend::Bootc => {
                let res = bootc::install(flatpak_id, progress).await;
                if res.requires_reboot {
                    self.show_toast("Reboot required to apply".into(), 1.0);
                }
                res.success
            }
        };

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        self.window.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();

        if success { self.refresh_installed().await; }
    }

    pub async fn remove(&self, flatpak_id: &str) {
        let backend = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| r.backend.clone()).unwrap_or(AppBackend::Flatpak)
        };
        let success = match backend {
            AppBackend::Flatpak => flatpak::remove(flatpak_id).await.success,
            AppBackend::Bootc   => bootc::remove(flatpak_id).await.success,
        };
        if success {
            self.refresh_installed().await;
            self.window.upgrade_in_event_loop(|w| w.set_detail_visible(false)).ok();
        }
    }

    // ── UI helpers (all ModelRc built on UI thread) ───────────────────────────

    /// Push apps + categories to UI. ModelRc built inside the closure = UI thread = safe.
    fn push_ui(&self, app_data: Vec<AppData>, cats: Vec<(String,String,String)>, loading: bool) {
        self.window.upgrade_in_event_loop(move |w| {
            // Build ModelRc on UI thread
            let entries: Vec<AppEntry> = app_data.into_iter().map(|d| d.into_entry()).collect();
            let app_model = ModelRc::new(VecModel::from(entries));
            w.set_apps(app_model);

            let cat_entries: Vec<CategoryEntry> = cats.into_iter().map(|(id, label, icon)| {
                CategoryEntry { id: id.into(), label: label.into(), icon: icon.into() }
            }).collect();
            let cat_model = ModelRc::new(VecModel::from(cat_entries));
            w.set_categories(cat_model);
            w.set_loading(loading);
        }).ok();
    }

    fn push_counts(&self, installed: i32, updates: i32) {
        self.window.upgrade_in_event_loop(move |w| {
            w.set_installed_count(installed);
            w.set_updates_count(updates);
        }).ok();
    }

    fn set_loading(&self, v: bool) {
        self.window.upgrade_in_event_loop(move |w| w.set_loading(v)).ok();
    }

    fn show_toast(&self, name: SharedString, progress: f32) {
        self.window.upgrade_in_event_loop(move |w| {
            w.set_toast_app(name);
            w.set_toast_progress(progress);
            w.set_toast_visible(true);
        }).ok();
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hex_to_color(hex: &str) -> Color {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 { return Color::from_rgb_u8(147, 51, 234); }
    Color::from_rgb_u8(
        u8::from_str_radix(&h[0..2], 16).unwrap_or(147),
        u8::from_str_radix(&h[2..4], 16).unwrap_or(51),
        u8::from_str_radix(&h[4..6], 16).unwrap_or(234),
    )
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
        "system"       => "System",
        _              => "Other",
    }
}
