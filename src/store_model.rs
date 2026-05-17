use std::sync::{Arc, Mutex};
use slint::{Color, ModelRc, SharedString, VecModel, Weak};
use crate::app_data::{AppBackend, AppRecord};
use crate::{flatpak, bootc};
use crate::{AppEntry, CategoryEntry, MainWindow, SuggestionEntry};

// ─── Page size ────────────────────────────────────────────────────────────────
const PAGE: usize = 40;

// ─── Send-safe mirror of AppEntry ────────────────────────────────────────────

#[derive(Clone)]
pub struct AppData {
    pub id:             SharedString,
    pub flatpak_id:     SharedString,
    pub name:           SharedString,
    pub summary:        SharedString,
    pub description:    SharedString,
    pub version:        SharedString,
    pub developer:      SharedString,
    pub category:       SharedString,
    pub icon_letter:    SharedString,
    pub icon_color:     Color,
    pub rating:         f32,
    pub download_count: SharedString,
    pub size_mb:        f32,
    pub is_installed:   bool,
    pub is_updating:    bool,
    pub backend:        SharedString,
    pub icon_url:       SharedString,
}

impl AppData {
    fn into_entry(self) -> AppEntry {
        AppEntry {
            id: self.id, flatpak_id: self.flatpak_id,
            name: self.name, summary: self.summary,
            description: self.description, version: self.version,
            developer: self.developer, category: self.category,
            icon_letter: self.icon_letter, icon_color: self.icon_color,
            rating: self.rating, download_count: self.download_count,
            size_mb: self.size_mb, is_installed: self.is_installed,
            is_updating: self.is_updating, backend: self.backend,
            icon_url: self.icon_url,
            tags: ModelRc::new(VecModel::from(vec![])),
        }
    }
}

// ─── Sort mode ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
pub enum SortMode { #[default] Installs, Name, Rating, Size, Recent }

impl SortMode {
    fn from_str(s: &str) -> Self {
        match s {
            "name"     => Self::Name,
            "rating"   => Self::Rating,
            "size"     => Self::Size,
            "recent"   => Self::Recent,
            _          => Self::Installs,
        }
    }
}

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct State {
    all_apps:       Vec<AppRecord>,
    installed_ids:  Vec<String>,
    bootc_layered:  Vec<String>,
    update_ids:     Vec<String>,
    search:         String,
    category:       String,
    sort:           SortMode,
    page:           usize,          // how many pages loaded (each = PAGE items)
    search_history: Vec<String>,    // last 8 queries
}

impl State {
    fn is_installed(&self, a: &AppRecord) -> bool {
        match a.backend {
            AppBackend::Flatpak => self.installed_ids.contains(&a.flatpak_id),
            AppBackend::Bootc   => self.bootc_layered.contains(&a.flatpak_id),
        }
    }

    fn is_updating(&self, a: &AppRecord) -> bool {
        self.update_ids.contains(&a.flatpak_id)
    }

    /// Returns (visible_slice, total_matching_count)
    fn filtered_sorted_paged(&self) -> (Vec<AppRecord>, usize) {
        let q   = self.search.to_lowercase();
        let cat = &self.category;

        let mut matched: Vec<(u32, &AppRecord)> = self.all_apps.iter()
            .filter_map(|a| {
                let installed = self.is_installed(a);
                let cat_ok = cat.is_empty() || cat == "all"
                    || cat == &a.category
                    || (cat == "installed" && installed)
                    || (cat == "updates"   && self.is_updating(a));
                if !cat_ok { return None; }

                let score = if q.is_empty() { base_score(a, &self.sort) }
                            else            { search_score(a, &q) };
                if score == 0 { return None; }
                Some((score, a))
            })
            .collect();

        // Sort
        match self.sort {
            SortMode::Name    => matched.sort_by(|a, b| a.1.name.cmp(&b.1.name)),
            SortMode::Rating  => matched.sort_by(|a, b| b.1.rating.partial_cmp(&a.1.rating).unwrap_or(std::cmp::Ordering::Equal)),
            SortMode::Size    => matched.sort_by(|a, b| b.1.size_mb.partial_cmp(&a.1.size_mb).unwrap_or(std::cmp::Ordering::Equal)),
            _                 => matched.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        let total = matched.len();
        let limit = PAGE * self.page.max(1);
        let slice: Vec<AppRecord> = matched.into_iter()
            .take(limit)
            .map(|(_, r)| r.clone())
            .collect();

        (slice, total)
    }

    fn to_app_data(&self, records: &[AppRecord]) -> Vec<AppData> {
        records.iter().map(|a| {
            AppData {
                id:             a.id.clone().into(),
                flatpak_id:     a.flatpak_id.clone().into(),
                name:           a.name.clone().into(),
                summary:        a.summary.clone().into(),
                description:    a.description.clone().into(),
                version:        a.version.clone().into(),
                developer:      a.developer.clone().into(),
                category:       category_label(&a.category).into(),
                icon_letter:    a.icon_letter.clone().into(),
                icon_color:     hex_to_color(&a.icon_color_hex),
                rating:         a.rating,
                download_count: a.download_count.clone().into(),
                size_mb:        a.size_mb,
                is_installed:   self.is_installed(a),
                is_updating:    self.is_updating(a),
                backend:        match a.backend { AppBackend::Flatpak => "flatpak", AppBackend::Bootc => "bootc" }.into(),
                icon_url:       "".into(),
            }
        }).collect()
    }

    fn featured(&self) -> Vec<AppRecord> {
        // Top 3 highest-rated Flatpak apps with >100K installs
        let mut cands: Vec<&AppRecord> = self.all_apps.iter()
            .filter(|a| a.backend == AppBackend::Flatpak && a.rating >= 4.5)
            .collect();
        cands.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal));
        cands.iter().take(3).map(|r| (*r).clone()).collect()
    }

    fn suggestions_for(&self, q: &str) -> Vec<(String, String, bool)> {
        // (display_text, app_id, is_recent)
        let ql = q.to_lowercase();
        let mut out: Vec<(String, String, bool)> = vec![];

        // Recent searches first
        for h in self.search_history.iter().rev() {
            if h.to_lowercase().contains(&ql) && out.len() < 3 {
                out.push((h.clone(), "".into(), true));
            }
        }

        // App name matches
        let mut apps: Vec<&AppRecord> = self.all_apps.iter()
            .filter(|a| {
                a.name.to_lowercase().contains(&ql)
                || a.flatpak_id.to_lowercase().contains(&ql)
                || a.developer.to_lowercase().contains(&ql)
            })
            .collect();
        apps.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal));

        for a in apps.iter().take(8 - out.len()) {
            out.push((a.name.clone(), a.id.clone(), false));
        }
        out
    }

    fn push_history(&mut self, q: &str) {
        if q.len() < 2 { return; }
        self.search_history.retain(|h| h != q);
        self.search_history.push(q.to_string());
        if self.search_history.len() > 8 { self.search_history.remove(0); }
    }
}

fn base_score(a: &AppRecord, sort: &SortMode) -> u32 {
    match sort {
        SortMode::Installs | SortMode::Recent => {
            // parse download_count like "2.1M", "890K"
            let s = &a.download_count;
            if s.ends_with('M') { (s[..s.len()-1].parse::<f32>().unwrap_or(0.0) * 1000.0) as u32 }
            else if s.ends_with('K') { s[..s.len()-1].parse::<f32>().unwrap_or(0.0) as u32 }
            else { s.parse().unwrap_or(1) }
        }
        SortMode::Rating  => (a.rating * 100.0) as u32,
        SortMode::Size    => a.size_mb as u32,
        SortMode::Name    => 1,
    }
}

fn search_score(a: &AppRecord, q: &str) -> u32 {
    let name = a.name.to_lowercase();
    let fid  = a.flatpak_id.to_lowercase();
    let sum  = a.summary.to_lowercase();
    let dev  = a.developer.to_lowercase();
    let cat  = a.category.to_lowercase();
    let desc = a.description.to_lowercase();

    if name == q             { return 10000; }
    if name.starts_with(q)   { return 9000; }
    if fid == q              { return 8500; }
    if fid.contains(q)       { return 8000; }
    if name.contains(q)      { return 7000; }
    if dev.contains(q)       { return 5000; }
    if sum.contains(q)       { return 4000; }
    if cat.contains(q)       { return 3000; }
    if desc.contains(q)      { return 1000; }

    // Token matching
    let hits = q.split_whitespace()
        .filter(|t| name.contains(t) || sum.contains(t) || fid.contains(t) || dev.contains(t))
        .count();
    if hits > 0 { (hits as u32 * 500).min(2500) } else { 0 }
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
                page: 1,
                ..Default::default()
            })),
            window,
        }
    }

    pub fn init(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_loading(true)).ok();
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

        // Push categories
        let cats = crate::catalog::categories();
        let state = self.state.lock().unwrap();
        let cat_counts: Vec<(String,String,String,i32)> = cats.into_iter().map(|(id, label, icon)| {
            let count = if id == "all" { state.all_apps.len() as i32 }
                        else { state.all_apps.iter().filter(|a| a.category == id).count() as i32 };
            (id, label, icon, count)
        }).collect();
        drop(state);

        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<CategoryEntry> = cat_counts.into_iter().map(|(id,label,icon,count)| {
                CategoryEntry { id: id.into(), label: label.into(), icon: icon.into(), count }
            }).collect();
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
        {
            let mut s = self.state.lock().unwrap();
            s.installed_ids = installed;
            s.update_ids    = updates;
            s.bootc_layered = layered;
        }
        self.push_all();
    }

    // ── Filter / Sort / Page ──────────────────────────────────────────────────

    pub fn filter_search(&self, q: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.search = q.to_string();
            s.page = 1;
        }
        self.push_all();
        // Suggestions
        if !q.is_empty() {
            let sugs = {
                let s = self.state.lock().unwrap();
                s.suggestions_for(q)
            };
            self.window.upgrade_in_event_loop(move |w| {
                let entries: Vec<SuggestionEntry> = sugs.into_iter().map(|(text, app_id, is_recent)| {
                    SuggestionEntry { text: text.into(), app_id: app_id.into(), is_recent }
                }).collect();
                w.set_suggestions(ModelRc::new(VecModel::from(entries)));
                w.set_show_suggestions(true);
                w.set_search_active(true);
            }).ok();
        } else {
            self.window.upgrade_in_event_loop(|w| {
                w.set_suggestions(ModelRc::new(VecModel::from(vec![])));
                w.set_show_suggestions(false);
                w.set_search_active(false);
            }).ok();
        }
    }

    pub fn filter_category(&self, cat: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.category = cat.to_string();
            s.page = 1;
        }
        self.push_all();
    }

    pub fn set_sort(&self, sort_str: &str) {
        {
            let mut s = self.state.lock().unwrap();
            s.sort = SortMode::from_str(sort_str);
            s.page = 1;
        }
        self.push_all();
    }

    pub fn load_more(&self) {
        {
            let mut s = self.state.lock().unwrap();
            s.page += 1;
        }
        self.push_all();
        self.window.upgrade_in_event_loop(|w| w.set_loading_more(false)).ok();
    }

    pub fn suggestion_selected(&self, id_or_query: &str) {
        // If it's an app id — open detail. Otherwise treat as search query.
        let found = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.id == id_or_query).map(|r| {
                let data = s.to_app_data(std::slice::from_ref(r));
                data.into_iter().next()
            }).flatten()
        };
        if let Some(d) = found {
            // Save to history
            { let mut s = self.state.lock().unwrap(); s.push_history(&d.name.to_string()); }
            self.window.upgrade_in_event_loop(move |w| {
                w.set_selected_app(d.into_entry());
                w.set_detail_visible(true);
                w.set_show_suggestions(false);
            }).ok();
        } else {
            // Treat as raw search query
            let q = id_or_query.to_string();
            { let mut s = self.state.lock().unwrap(); s.search = q.clone(); s.page = 1; s.push_history(&q); }
            self.push_all();
            let qs: SharedString = id_or_query.into();
            self.window.upgrade_in_event_loop(move |w| {
                w.set_search_text(qs);
                w.set_search_active(true);
                w.set_show_suggestions(false);
            }).ok();
        }
    }

    pub fn open_detail(&self, id: &str) {
        let data = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.id == id).map(|r| {
                let v = s.to_app_data(std::slice::from_ref(r));
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

    // ── Install / Remove / Update ─────────────────────────────────────────────

    pub async fn install(&self, flatpak_id: &str) {
        let (name, backend) = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| (r.name.clone(), r.backend.clone()))
                .unwrap_or_else(|| (flatpak_id.to_string(), AppBackend::Flatpak))
        };

        self.show_toast(name.into(), 0.0, "");

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
                    self.show_toast("".into(), 1.0, "Reboot to finalise");
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

    pub async fn update_app(&self, flatpak_id: &str) {
        self.show_toast(flatpak_id.into(), 0.0, "Updating…");
        let result = flatpak::update(flatpak_id).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        self.window.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();
        if result.success { self.refresh_installed().await; }
    }

    pub async fn update_all(&self) {
        self.show_toast("All apps".into(), 0.0, "Updating all…");
        let ids: Vec<String> = {
            let s = self.state.lock().unwrap();
            s.update_ids.clone()
        };
        let total = ids.len() as f32;
        for (i, id) in ids.iter().enumerate() {
            let p = (i as f32 + 1.0) / total;
            flatpak::update(id).await;
            let prog = p;
            self.window.upgrade_in_event_loop(move |w| w.set_toast_progress(prog)).ok();
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        self.window.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();
        self.refresh_installed().await;
    }

    // ── UI push (all ModelRc on UI thread) ────────────────────────────────────

    fn push_all(&self) {
        let (slice, total, featured_data, ic, uc) = {
            let s = self.state.lock().unwrap();
            let (slice, total) = s.filtered_sorted_paged();
            let featured = s.featured();
            let feat_data = s.to_app_data(&featured);
            let data = s.to_app_data(&slice);
            let ic = s.installed_ids.len() as i32;
            let uc = s.update_ids.len() as i32;
            (data, total as i32, feat_data, ic, uc)
        };

        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<AppEntry> = slice.into_iter().map(|d| d.into_entry()).collect();
            w.set_apps(ModelRc::new(VecModel::from(entries)));

            let feat: Vec<AppEntry> = featured_data.into_iter().map(|d| d.into_entry()).collect();
            w.set_featured_apps(ModelRc::new(VecModel::from(feat)));

            w.set_total_count(total);
            w.set_installed_count(ic);
            w.set_updates_count(uc);
            w.set_loading(false);
            w.set_loading_more(false);
        }).ok();
    }

    fn show_toast(&self, name: SharedString, progress: f32, status: &'static str) {
        self.window.upgrade_in_event_loop(move |w| {
            w.set_toast_app(name);
            w.set_toast_progress(progress);
            w.set_toast_status(status.into());
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
