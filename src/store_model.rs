use std::sync::{Arc, Mutex};
use slint::{Color, ModelRc, SharedString, VecModel, Weak};
use crate::app_data::{AppBackend, AppRecord};
use crate::{flatpak, bootc};
use crate::{AppEntry, CategoryEntry, MainWindow};

// ─── Send-safe mirror of AppEntry (no Rc inside) ─────────────────────────────

#[derive(Clone)]
struct AppData {
    id:             SharedString,
    flatpak_id:     SharedString,
    name:           SharedString,
    summary:        SharedString,
    description:    SharedString,
    version:        SharedString,
    developer:      SharedString,
    category:       SharedString,
    icon_letter:    SharedString,
    icon_color:     Color,
    rating:         f32,
    download_count: SharedString,
    size_mb:        f32,
    is_installed:   bool,
    is_updating:    bool,
    backend:        SharedString,
}

impl AppData {
    /// Build AppEntry on the UI thread (ModelRc is safe here)
    fn into_entry(self) -> AppEntry {
        AppEntry {
            id: self.id, flatpak_id: self.flatpak_id,
            name: self.name, summary: self.summary,
            description: self.description, version: self.version,
            developer: self.developer, category: self.category,
            icon_letter: self.icon_letter, icon_color: self.icon_color,
            rating: self.rating, download_count: self.download_count,
            size_mb: self.size_mb,
            is_installed: self.is_installed, is_updating: self.is_updating,
            backend: self.backend,
            tags: ModelRc::new(VecModel::from(vec![])),
        }
    }
}

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct State {
    all_apps:       Vec<AppRecord>,
    installed_ids:  Vec<String>,  // Flatpak reverse-DNS IDs
    bootc_layered:  Vec<String>,  // RPM package names
    update_ids:     Vec<String>,
    search:         String,
    category:       String,
}

impl State {
    fn is_installed(&self, a: &AppRecord) -> bool {
        match a.backend {
            AppBackend::Flatpak => self.installed_ids.contains(&a.flatpak_id),
            AppBackend::Bootc   => self.bootc_layered.contains(&a.flatpak_id),
        }
    }

    /// Multi-field fuzzy search: score each app, sort descending.
    /// Returns apps with score > 0 (or all if query empty).
    fn filtered(&self) -> Vec<AppRecord> {
        let q = self.search.to_lowercase();
        let cat = &self.category;

        let mut scored: Vec<(u32, AppRecord)> = self.all_apps.iter()
            .filter_map(|a| {
                let installed = self.is_installed(a);

                // Category gate
                let cat_ok = cat.is_empty() || cat == "all"
                    || cat == &a.category
                    || (cat == "installed" && installed)
                    || (cat == "updates" && self.update_ids.contains(&a.flatpak_id));
                if !cat_ok { return None; }

                // Score
                let score = if q.is_empty() {
                    1u32  // no search — show everything
                } else {
                    score_app(a, &q)
                };
                if score == 0 { return None; }

                Some((score, a.clone()))
            })
            .collect();

        // Sort by relevance (desc), then name (asc) for ties
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    fn to_app_data(&self, records: Vec<AppRecord>) -> Vec<AppData> {
        records.into_iter().map(|a| {
            let is_installed = self.is_installed(&a);
            let is_updating  = self.update_ids.contains(&a.flatpak_id);
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
                    AppBackend::Flatpak => "flatpak",
                    AppBackend::Bootc   => "bootc",
                }.into(),
            }
        }).collect()
    }
}

/// Relevance scoring: returns 0 = no match, higher = better match.
fn score_app(a: &AppRecord, q: &str) -> u32 {
    let name   = a.name.to_lowercase();
    let sum    = a.summary.to_lowercase();
    let dev    = a.developer.to_lowercase();
    let fid    = a.flatpak_id.to_lowercase();
    let cat    = a.category.to_lowercase();
    let desc   = a.description.to_lowercase();

    // Exact name match
    if name == q                   { return 1000; }
    // Name starts with query
    if name.starts_with(q)         { return 900; }
    // Flatpak ID exact
    if fid == q                    { return 850; }
    // Flatpak ID contains (e.g. "org.mozilla.firefox" for "firefox")
    if fid.contains(q)             { return 800; }
    // Name contains
    if name.contains(q)            { return 700; }
    // Developer contains
    if dev.contains(q)             { return 500; }
    // Summary contains
    if sum.contains(q)             { return 400; }
    // Category contains
    if cat.contains(q)             { return 300; }
    // Description contains
    if desc.contains(q)            { return 100; }

    // Partial token matching: split query into words, score each
    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.len() > 1 {
        let hits = tokens.iter().filter(|t|
            name.contains(*t) || sum.contains(*t) || fid.contains(*t) || dev.contains(*t)
        ).count();
        if hits > 0 { return (hits as u32 * 50).min(250); }
    }

    0
}

// ─── StoreModel ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct StoreModel {
    state:  Arc<Mutex<State>>,
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
        // Show loading state immediately
        self.window.upgrade_in_event_loop(|w| {
            w.set_loading(true);
            w.set_categories(ModelRc::new(VecModel::from(vec![])));
        }).ok();

        let m = self.clone();
        tokio::spawn(async move {
            m.load_catalog().await;
            m.refresh_installed().await;
        });
    }

    // ── Catalog ───────────────────────────────────────────────────────────────

    async fn load_catalog(&self) {
        let (fp, bc) = tokio::join!(
            crate::catalog::fetch_flatpak_apps(),
            crate::catalog::fetch_bootc_packages(),
        );
        let mut all = fp;
        all.extend(bc);
        {
            let mut s = self.state.lock().unwrap();
            s.all_apps = all;
        }
        // Push categories once catalog is ready
        let cats = crate::catalog::categories();
        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<CategoryEntry> = cats.into_iter()
                .map(|(id, label, icon)| CategoryEntry { id: id.into(), label: label.into(), icon: icon.into() })
                .collect();
            w.set_categories(ModelRc::new(VecModel::from(entries)));
        }).ok();
    }

    // ── Installed refresh ─────────────────────────────────────────────────────

    pub async fn refresh_installed(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_loading(true)).ok();

        let (installed, updates, layered) = tokio::join!(
            flatpak::list_installed(),
            flatpak::list_updates(),
            bootc::list_packages(),
        );

        let (data, installed_count, upd_count) = {
            let mut s = self.state.lock().unwrap();
            s.installed_ids = installed;
            s.update_ids    = updates;
            s.bootc_layered = layered;
            let filtered = s.filtered();
            let data     = s.to_app_data(filtered);
            let ic = data.iter().filter(|a| a.is_installed).count() as i32;
            let uc = s.update_ids.len() as i32;
            (data, ic, uc)
        };

        self.push_apps(data, installed_count, upd_count, false);
    }

    // ── Filter ────────────────────────────────────────────────────────────────

    pub fn filter_search(&self, query: &str) {
        let query = query.to_string();
        {
            let mut s = self.state.lock().unwrap();
            s.search = query.clone();
        }
        self.push_filtered();
        // Update search-active flag on UI
        let active = !query.is_empty();
        let q = query.clone();
        self.window.upgrade_in_event_loop(move |w| {
            w.set_search_active(active);
            if !active { w.set_search_text("".into()); }
        }).ok();
    }

    pub fn filter_category(&self, cat: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.category = cat.to_string();
        }
        self.push_filtered();
    }

    fn push_filtered(&self) {
        let (data, ic, uc) = {
            let s = self.state.lock().unwrap();
            let filtered = s.filtered();
            let data = s.to_app_data(filtered);
            let ic = s.installed_ids.len() as i32;
            let uc = s.update_ids.len() as i32;
            (data, ic, uc)
        };
        self.push_apps(data, ic, uc, false);
    }

    // ── Detail ────────────────────────────────────────────────────────────────

    pub fn open_detail(&self, id: &str) {
        let data = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.id == id).map(|r| {
                s.to_app_data(vec![r.clone()]).into_iter().next()
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
        let (name, backend) = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| (r.name.clone(), r.backend.clone()))
                .unwrap_or_else(|| (flatpak_id.to_string(), AppBackend::Flatpak))
        };

        self.show_toast(name.clone().into(), 0.0);

        let ww = self.window.clone();
        let progress = move |p: f32| {
            let w = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = w.upgrade() { win.set_toast_progress(p); }
            });
        };

        let success = match backend {
            AppBackend::Flatpak => flatpak::install(flatpak_id, progress).await.success,
            AppBackend::Bootc   => {
                let res = bootc::install(flatpak_id, progress).await;
                if res.requires_reboot {
                    self.show_toast("Reboot to finalise installation".into(), 1.0);
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
                .map(|r| r.backend.clone())
                .unwrap_or(AppBackend::Flatpak)
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

    // ── UI helpers ────────────────────────────────────────────────────────────

    fn push_apps(&self, data: Vec<AppData>, installed: i32, updates: i32, loading: bool) {
        self.window.upgrade_in_event_loop(move |w| {
            let total = data.len() as i32;
            let entries: Vec<AppEntry> = data.into_iter().map(|d| d.into_entry()).collect();
            w.set_apps(ModelRc::new(VecModel::from(entries)));
            w.set_installed_count(installed);
            w.set_updates_count(updates);
            w.set_total_results(total);
            w.set_loading(loading);
        }).ok();
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
