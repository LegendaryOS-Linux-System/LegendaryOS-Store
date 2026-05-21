use std::sync::{Arc, Mutex};
use slint::{Color, ModelRc, SharedString, VecModel, Weak};
use crate::app_data::{AppBackend, AppRecord};
use crate::{flatpak, bootc};
use crate::{
    AppEntry, BootcStatusEntry, CategoryEntry, DiskUsageEntry,
    MainWindow, QueueEntry, RecentEntry, RemoteEntry, SuggestionEntry,
};

const PAGE: usize = 40;

// ─── Queue ────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum QueueStatus { Pending, Running, Done, Error }

#[derive(Clone)]
pub struct QueueItem {
    pub app_id:   String,
    pub name:     String,
    pub action:   String,
    pub progress: f32,
    pub status:   QueueStatus,
}

impl QueueItem {
    fn to_entry(&self) -> QueueEntry {
        QueueEntry {
            app_id:   self.app_id.clone().into(),
            name:     self.name.clone().into(),
            action:   self.action.clone().into(),
            progress: self.progress,
            status: match self.status {
                QueueStatus::Pending => "pending",
                QueueStatus::Running => "running",
                QueueStatus::Done    => "done",
                QueueStatus::Error   => "error",
            }.into(),
        }
    }
}

// ─── Recent activity ─────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RecentActivity {
    pub app_id:      String,
    pub name:        String,
    pub action:      String,   // install | remove | update
    pub icon_letter: String,
    pub icon_color:  String,
    pub when:        String,
}

// ─── Send-safe AppData ────────────────────────────────────────────────────────

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
    pub installed_size: f32,
    pub is_installed:   bool,
    pub is_updating:    bool,
    pub is_selected:    bool,
    pub backend:        SharedString,
    pub icon_url:       SharedString,
    pub website:        SharedString,
    pub changelog:      SharedString,
    pub permissions:    Vec<SharedString>,
}

impl AppData {
    fn into_entry(self) -> AppEntry {
        let perms = self.permissions.clone();
        AppEntry {
            id: self.id, flatpak_id: self.flatpak_id,
            name: self.name, summary: self.summary,
            description: self.description, version: self.version,
            developer: self.developer, category: self.category,
            icon_letter: self.icon_letter, icon_color: self.icon_color,
            rating: self.rating, download_count: self.download_count,
            size_mb: self.size_mb, installed_size: self.installed_size,
            is_installed: self.is_installed, is_updating: self.is_updating,
            is_selected: self.is_selected, backend: self.backend,
            icon_url: self.icon_url, website: self.website,
            changelog: self.changelog,
            tags:        ModelRc::new(VecModel::from(vec![])),
            permissions: ModelRc::new(VecModel::from(perms)),
        }
    }
}

// ─── Sort ─────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Default)]
pub enum SortMode { #[default] Installs, Name, Rating, Size }

impl SortMode {
    fn from_str(s: &str) -> Self {
        match s { "name" => Self::Name, "rating" => Self::Rating, "size" => Self::Size, _ => Self::Installs }
    }
}

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct State {
    all_apps:        Vec<AppRecord>,
    installed_ids:   Vec<String>,
    bootc_layered:   Vec<String>,
    update_ids:      Vec<String>,
    selected_ids:    Vec<String>,
    search:          String,
    category:        String,
    sort:            SortMode,
    page:            usize,
    search_history:  Vec<String>,
    queue:           Vec<QueueItem>,
    recent:          Vec<RecentActivity>,
    permissions:     std::collections::HashMap<String, Vec<String>>,
    installed_sizes: std::collections::HashMap<String, f32>,
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

    fn filtered_sorted_paged(&self) -> (Vec<AppRecord>, usize) {
        let q   = self.search.to_lowercase();
        let cat = &self.category;

        let mut matched: Vec<(u32, &AppRecord)> = self.all_apps.iter()
            .filter_map(|a| {
                let inst = self.is_installed(a);
                let cat_ok = cat.is_empty() || cat == "all"
                    || cat == &a.category
                    || (cat == "installed" && inst)
                    || (cat == "updates"   && self.is_updating(a));
                if !cat_ok { return None; }
                let score = if q.is_empty() { base_score(a, &self.sort) } else { search_score(a, &q) };
                if score == 0 { None } else { Some((score, a)) }
            })
            .collect();

        match self.sort {
            SortMode::Name   => matched.sort_by(|a, b| a.1.name.cmp(&b.1.name)),
            SortMode::Rating => matched.sort_by(|a, b| b.1.rating.partial_cmp(&a.1.rating).unwrap_or(std::cmp::Ordering::Equal)),
            SortMode::Size   => matched.sort_by(|a, b| b.1.size_mb.partial_cmp(&a.1.size_mb).unwrap_or(std::cmp::Ordering::Equal)),
            _                => matched.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        let total = matched.len();
        let limit = PAGE * self.page.max(1);
        (matched.into_iter().take(limit).map(|(_, r)| r.clone()).collect(), total)
    }

    fn to_app_data(&self, records: &[AppRecord]) -> Vec<AppData> {
        records.iter().map(|a| {
            let perms = self.permissions.get(&a.flatpak_id)
                .map(|v| v.iter().map(|s| -> SharedString { s.clone().into() }).collect())
                .unwrap_or_default();
            let installed_size = self.installed_sizes.get(&a.flatpak_id).copied().unwrap_or(0.0);
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
                installed_size,
                is_installed:   self.is_installed(a),
                is_updating:    self.is_updating(a),
                is_selected:    self.selected_ids.contains(&a.id),
                backend:        match a.backend { AppBackend::Flatpak => "flatpak", AppBackend::Bootc => "bootc" }.into(),
                icon_url:       a.icon_url.clone().into(),
                website:        "".into(),
                changelog:      "".into(),
                permissions:    perms,
            }
        }).collect()
    }

    fn featured(&self) -> Vec<AppRecord> {
        let mut cands: Vec<&AppRecord> = self.all_apps.iter()
            .filter(|a| a.backend == AppBackend::Flatpak && a.rating >= 4.5)
            .collect();
        cands.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal));
        cands.iter().take(3).map(|r| (*r).clone()).collect()
    }

    fn suggestions_for(&self, q: &str) -> Vec<(String, String, bool)> {
        let ql = q.to_lowercase();
        let mut out = vec![];
        for h in self.search_history.iter().rev() {
            if h.to_lowercase().contains(&ql) && out.len() < 3 {
                out.push((h.clone(), "".into(), true));
            }
        }
        let mut apps: Vec<&AppRecord> = self.all_apps.iter()
            .filter(|a| a.name.to_lowercase().contains(&ql) || a.flatpak_id.to_lowercase().contains(&ql) || a.developer.to_lowercase().contains(&ql))
            .collect();
        apps.sort_by(|a, b| b.rating.partial_cmp(&a.rating).unwrap_or(std::cmp::Ordering::Equal));
        for a in apps.iter().take(8usize.saturating_sub(out.len())) {
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

    fn queue_count_active(&self) -> i32 {
        self.queue.iter().filter(|q| q.status == QueueStatus::Pending || q.status == QueueStatus::Running).count() as i32
    }

    fn add_recent(&mut self, app_id: &str, name: &str, action: &str, icon_letter: &str, icon_color: &str) {
        // Remove duplicates for same app
        self.recent.retain(|r| r.app_id != app_id);
        self.recent.insert(0, RecentActivity {
            app_id:      app_id.to_string(),
            name:        name.to_string(),
            action:      action.to_string(),
            icon_letter: icon_letter.to_string(),
            icon_color:  icon_color.to_string(),
            when:        "just now".to_string(),
        });
        if self.recent.len() > 8 { self.recent.truncate(8); }
    }
}

fn base_score(a: &AppRecord, _: &SortMode) -> u32 {
    let s = &a.download_count;
    if s.ends_with('M')      { (s[..s.len()-1].parse::<f32>().unwrap_or(0.0) * 1000.0) as u32 }
    else if s.ends_with('K') { s[..s.len()-1].parse::<f32>().unwrap_or(0.0) as u32 }
    else                     { s.parse().unwrap_or(1) }
}

fn search_score(a: &AppRecord, q: &str) -> u32 {
    let name = a.name.to_lowercase();
    let fid  = a.flatpak_id.to_lowercase();
    let sum  = a.summary.to_lowercase();
    let dev  = a.developer.to_lowercase();
    let desc = a.description.to_lowercase();
    if name == q           { return 10000; }
    if name.starts_with(q) { return 9000; }
    if fid == q            { return 8500; }
    if fid.contains(q)     { return 8000; }
    if name.contains(q)    { return 7000; }
    if dev.contains(q)     { return 5000; }
    if sum.contains(q)     { return 4000; }
    if desc.contains(q)    { return 1000; }
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
            state: Arc::new(Mutex::new(State { category: "all".into(), page: 1, ..Default::default() })),
            window,
        }
    }

    pub fn init(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_loading(true)).ok();
        let m = self.clone();
        tokio::spawn(async move {
            m.load_catalog().await;
            // Parallel: refresh installed + bootc status + remotes
            let (_, _, _) = tokio::join!(
                m.refresh_installed_inner(),
                m.refresh_bootc_status(),
                m.refresh_remotes_inner(),
            );
            m.push_all();
        });
    }

    // ── Catalog ───────────────────────────────────────────────────────────────

    async fn load_catalog(&self) {
        let (fp, bc) = tokio::join!(
            crate::catalog::fetch_flatpak_apps(),
            crate::catalog::fetch_bootc_packages(),
        );
        let mut all = fp; all.extend(bc);
        { let mut s = self.state.lock().unwrap(); s.all_apps = all; }

        let cats = crate::catalog::categories();
        let cat_counts: Vec<(String, String, String, i32)> = {
            let s = self.state.lock().unwrap();
            cats.into_iter().map(|(id, label, icon)| {
                let count = if id == "all" { s.all_apps.len() as i32 }
                            else { s.all_apps.iter().filter(|a| a.category == id).count() as i32 };
                (id, label, icon, count)
            }).collect()
        };

        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<CategoryEntry> = cat_counts.into_iter().map(|(id, label, icon, count)| {
                CategoryEntry { id: id.into(), label: label.into(), icon: icon.into(), count }
            }).collect();
            w.set_categories(ModelRc::new(VecModel::from(entries)));
        }).ok();
    }

    // ── Installed refresh ─────────────────────────────────────────────────────

    pub async fn refresh_installed(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_loading(true)).ok();
        self.refresh_installed_inner().await;
        self.push_all();
    }

    async fn refresh_installed_inner(&self) {
        let (installed, updates, layered) = tokio::join!(
            flatpak::list_installed(),
            flatpak::list_updates(),
            bootc::list_packages(),
        );
        let sizes = flatpak::installed_sizes(&installed).await;
        {
            let mut s = self.state.lock().unwrap();
            s.installed_ids  = installed;
            s.update_ids     = updates;
            s.bootc_layered  = layered;
            s.installed_sizes = sizes;
        }
    }

    // ── bootc status ─────────────────────────────────────────────────────────

    pub async fn refresh_bootc_status(&self) {
        let st = bootc::status().await;
        let reboot = st.reboot_required || st.staged_image.is_some();
        let layered_count = st.layered_packages.len() as i32;

        let entry = BootcStatusEntry {
            booted_image:    st.booted_image.into(),
            booted_version:  st.booted_version.into(),
            staged_image:    st.staged_image.unwrap_or_default().into(),
            layered_count,
            reboot_required: reboot,
            timestamp:       st.timestamp.into(),
        };

        self.window.upgrade_in_event_loop(move |w| {
            w.set_bootc_status(entry);
            w.set_reboot_pending(reboot);
            w.set_bootc_upgrading(false);
        }).ok();

        // Show reboot notification if pending
        if reboot {
            self.show_notification(
                "Staged deployment ready — reboot to activate".to_string(),
                "warning",
            );
        }
    }

    // ── Remotes ───────────────────────────────────────────────────────────────

    pub async fn refresh_remotes_inner(&self) {
        let remotes = list_flatpak_remotes().await;
        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<RemoteEntry> = remotes.into_iter().map(|(name, url, enabled)| {
                RemoteEntry { name: name.into(), url: url.into(), enabled }
            }).collect();
            w.set_remotes(ModelRc::new(VecModel::from(entries)));
        }).ok();
    }

    pub async fn refresh_remotes(&self) {
        self.refresh_remotes_inner().await;
    }

    pub async fn toggle_remote(&self, name: &str) {
        // flatpak remote-modify --enable/--disable
        let remotes = list_flatpak_remotes().await;
        let currently_enabled = remotes.iter().find(|(n, _, _)| n == name).map(|(_, _, e)| *e).unwrap_or(false);
        let flag = if currently_enabled { "--disable" } else { "--enable" };
        let _ = tokio::process::Command::new("flatpak")
            .args(["remote-modify", flag, name])
            .output().await;
        self.refresh_remotes_inner().await;
    }

    pub async fn add_remote(&self, name: &str, url: &str) {
        let _ = tokio::process::Command::new("flatpak")
            .args(["remote-add", "--if-not-exists", "--user", name, url])
            .output().await;
        self.refresh_remotes_inner().await;
        self.show_notification(format!("Added repository: {name}"), "success");
    }

    pub async fn remove_remote(&self, name: &str) {
        let _ = tokio::process::Command::new("flatpak")
            .args(["remote-delete", "--user", name])
            .output().await;
        self.refresh_remotes_inner().await;
    }

    // ── bootc operations ──────────────────────────────────────────────────────

    pub async fn bootc_upgrade(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_bootc_upgrading(true)).ok();
        self.show_notification("Pulling latest LegendaryOS base image…".to_string(), "info");

        let result = bootc::bootc_upgrade().await;

        self.window.upgrade_in_event_loop(|w| w.set_bootc_upgrading(false)).ok();

        if result.success {
            self.show_notification("Base image pulled — reboot to activate".to_string(), "success");
        } else {
            self.show_notification("bootc upgrade failed — check logs".to_string(), "error");
        }
        self.refresh_bootc_status().await;
    }

    pub async fn rpmostree_upgrade(&self) {
        self.show_notification("Running rpm-ostree upgrade…".to_string(), "info");
        let result = bootc::rpmostree_upgrade().await;
        if result.success {
            self.show_notification("rpm-ostree upgrade complete — reboot to activate".to_string(), "success");
        } else {
            self.show_notification("rpm-ostree upgrade failed".to_string(), "error");
        }
        self.refresh_bootc_status().await;
    }

    pub async fn bootc_rollback(&self) {
        self.show_notification("Rolling back to previous deployment…".to_string(), "warning");
        let result = bootc::rollback().await;
        if result.success {
            self.show_notification("Rollback staged — reboot to activate".to_string(), "success");
        } else {
            self.show_notification("Rollback failed".to_string(), "error");
        }
        self.refresh_bootc_status().await;
    }

    // ── Filter / sort / page ──────────────────────────────────────────────────

    pub fn filter_search(&self, q: &str) {
        let q = q.to_string();
        { let mut s = self.state.lock().unwrap(); s.search = q.clone(); s.page = 1; }
        self.push_all();

        if !q.is_empty() {
            let sugs = { let s = self.state.lock().unwrap(); s.suggestions_for(&q) };
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
        { let mut s = self.state.lock().unwrap(); s.category = cat.to_string(); s.page = 1; }
        self.push_all();
    }

    pub fn set_sort(&self, sort_str: &str) {
        { let mut s = self.state.lock().unwrap(); s.sort = SortMode::from_str(sort_str); s.page = 1; }
        self.push_all();
    }

    pub fn load_more(&self) {
        { let mut s = self.state.lock().unwrap(); s.page += 1; }
        self.push_all();
        self.window.upgrade_in_event_loop(|w| w.set_loading_more(false)).ok();
    }

    pub fn suggestion_selected(&self, id_or_query: &str) {
        let found = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.id == id_or_query).map(|r| {
                s.to_app_data(std::slice::from_ref(r)).into_iter().next()
            }).flatten()
        };
        if let Some(d) = found {
            { let mut s = self.state.lock().unwrap(); s.push_history(&d.name.to_string()); }
            self.window.upgrade_in_event_loop(move |w| {
                w.set_selected_app(d.into_entry());
                w.set_detail_visible(true);
                w.set_show_suggestions(false);
            }).ok();
        } else {
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
                s.to_app_data(std::slice::from_ref(r)).into_iter().next()
            }).flatten()
        };
        if let Some(d) = data {
            let fid     = d.flatpak_id.to_string();
            let backend = d.backend.to_string();
            self.window.upgrade_in_event_loop(move |w| {
                w.set_selected_app(d.into_entry());
                w.set_detail_visible(true);
                w.set_detail_tab(0);
            }).ok();
            if backend == "flatpak" {
                let m = self.clone();
                tokio::spawn(async move { m.fetch_permissions_for(&fid).await; });
            }
        }
    }

    async fn fetch_permissions_for(&self, flatpak_id: &str) {
        let perms = get_app_permissions(flatpak_id).await;
        let fid   = flatpak_id.to_string();
        { let mut s = self.state.lock().unwrap(); s.permissions.insert(fid.clone(), perms); }
        let updated = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == fid).map(|r| {
                s.to_app_data(std::slice::from_ref(r)).into_iter().next()
            }).flatten()
        };
        if let Some(d) = updated {
            self.window.upgrade_in_event_loop(move |w| {
                if w.get_selected_app().flatpak_id.as_str() == d.flatpak_id.as_str() {
                    w.set_selected_app(d.into_entry());
                }
            }).ok();
        }
    }

    // ── Batch selection ───────────────────────────────────────────────────────

    pub fn toggle_select(&self, id: &str) {
        {
            let mut s = self.state.lock().unwrap();
            if s.selected_ids.contains(&id.to_string()) {
                s.selected_ids.retain(|i| i != id);
            } else {
                s.selected_ids.push(id.to_string());
            }
        }
        self.push_all();
    }

    pub fn select_all_visible(&self) {
        {
            let mut s = self.state.lock().unwrap();
            let (visible, _) = s.filtered_sorted_paged();
            for a in &visible {
                if !s.selected_ids.contains(&a.id) { s.selected_ids.push(a.id.clone()); }
            }
        }
        self.push_all();
    }

    pub fn clear_selection(&self) {
        { let mut s = self.state.lock().unwrap(); s.selected_ids.clear(); }
        self.push_all();
    }

    // ── Batch operations ──────────────────────────────────────────────────────

    pub async fn batch_install(&self) {
        let ops: Vec<_> = {
            let s = self.state.lock().unwrap();
            s.selected_ids.iter().filter_map(|id| {
                s.all_apps.iter().find(|a| &a.id == id && !s.is_installed(a))
                    .map(|a| (a.flatpak_id.clone(), a.name.clone(), "install".to_string()))
            }).collect()
        };
        self.run_queue(ops).await;
    }

    pub async fn batch_remove(&self) {
        let ops: Vec<_> = {
            let s = self.state.lock().unwrap();
            s.selected_ids.iter().filter_map(|id| {
                s.all_apps.iter().find(|a| &a.id == id && s.is_installed(a))
                    .map(|a| (a.flatpak_id.clone(), a.name.clone(), "remove".to_string()))
            }).collect()
        };
        self.run_queue(ops).await;
    }

    pub async fn batch_update(&self) {
        let ops: Vec<_> = {
            let s = self.state.lock().unwrap();
            s.selected_ids.iter().filter_map(|id| {
                s.all_apps.iter().find(|a| &a.id == id && s.is_updating(a))
                    .map(|a| (a.flatpak_id.clone(), a.name.clone(), "update".to_string()))
            }).collect()
        };
        self.run_queue(ops).await;
    }

    async fn run_queue(&self, items: Vec<(String, String, String)>) {
        {
            let mut s = self.state.lock().unwrap();
            for (fid, name, action) in &items {
                s.queue.push(QueueItem {
                    app_id: fid.clone(), name: name.clone(), action: action.clone(),
                    progress: 0.0, status: QueueStatus::Pending,
                });
            }
        }
        self.push_queue();

        for (fid, name, action) in items {
            self.set_queue_status(&fid, QueueStatus::Running, 0.0);
            let backend = {
                let s = self.state.lock().unwrap();
                s.all_apps.iter().find(|a| a.flatpak_id == fid)
                    .map(|r| r.backend.clone()).unwrap_or(AppBackend::Flatpak)
            };
            let ww = self.window.clone();
            let fid_c = fid.clone();
            let progress = move |p: f32| {
                let w = ww.clone(); let fc = fid_c.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() { win.set_toast_progress(p); win.set_toast_app(fc.into()); }
                });
            };
            let success = match action.as_str() {
                "install" => match backend {
                    AppBackend::Flatpak => flatpak::install(&fid, progress).await.success,
                    AppBackend::Bootc   => bootc::install(&fid, |_| {}).await.success,
                },
                "remove" => match backend {
                    AppBackend::Flatpak => flatpak::remove(&fid).await.success,
                    AppBackend::Bootc   => bootc::remove(&fid).await.success,
                },
                "update" => flatpak::update(&fid).await.success,
                _ => false,
            };
            self.set_queue_status(&fid, if success { QueueStatus::Done } else { QueueStatus::Error }, 1.0);

            // Add to recent activity
            if success {
                let (icon_letter, icon_color) = {
                    let s = self.state.lock().unwrap();
                    s.all_apps.iter().find(|a| a.flatpak_id == fid)
                        .map(|a| (a.icon_letter.clone(), a.icon_color_hex.clone()))
                        .unwrap_or_else(|| (fid[..1].to_uppercase(), "#7c3aed".into()))
                };
                let mut s = self.state.lock().unwrap();
                s.add_recent(&fid, &name, &action, &icon_letter, &icon_color);
            }
            self.push_queue();
        }

        { let mut s = self.state.lock().unwrap(); s.selected_ids.clear(); }
        self.refresh_installed().await;

        if !{ self.state.lock().unwrap().queue.iter().any(|q| q.status == QueueStatus::Error) } {
            self.show_notification("All operations completed successfully".to_string(), "success");
        } else {
            self.show_notification("Some operations failed — check the queue".to_string(), "error");
        }
    }

    fn set_queue_status(&self, app_id: &str, status: QueueStatus, progress: f32) {
        let mut s = self.state.lock().unwrap();
        if let Some(item) = s.queue.iter_mut().find(|q| q.app_id == app_id) {
            item.status = status; item.progress = progress;
        }
    }

    pub fn cancel_queue_item(&self, app_id: &str) {
        { let mut s = self.state.lock().unwrap(); s.queue.retain(|q| q.app_id != app_id || q.status == QueueStatus::Running); }
        self.push_queue();
    }

    pub fn clear_done_queue(&self) {
        { let mut s = self.state.lock().unwrap(); s.queue.retain(|q| q.status != QueueStatus::Done && q.status != QueueStatus::Error); }
        self.push_queue();
    }

    fn push_queue(&self) {
        let (items, count) = {
            let s = self.state.lock().unwrap();
            (s.queue.clone(), s.queue_count_active())
        };
        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<QueueEntry> = items.iter().map(|i| i.to_entry()).collect();
            w.set_queue(ModelRc::new(VecModel::from(entries)));
            w.set_queue_count(count);
        }).ok();
    }

    // ── Single install / remove / update ──────────────────────────────────────

    pub async fn install(&self, flatpak_id: &str) {
        let (name, backend, icon_letter, icon_color) = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| (r.name.clone(), r.backend.clone(), r.icon_letter.clone(), r.icon_color_hex.clone()))
                .unwrap_or_else(|| (flatpak_id.to_string(), AppBackend::Flatpak, "?".into(), "#7c3aed".into()))
        };
        self.show_toast(name.clone().into(), 0.0, "");
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
                if res.requires_reboot { self.show_toast("".into(), 1.0, "Reboot to finalise"); }
                res.success
            }
        };
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        self.window.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();
        if success {
            {
                let mut s = self.state.lock().unwrap();
                s.add_recent(flatpak_id, &name, "install", &icon_letter, &icon_color);
            }
            self.push_recent();
            self.show_notification(format!("{name} installed successfully"), "success");
            self.refresh_installed().await;
        } else {
            self.show_notification(format!("Failed to install {name}"), "error");
        }
    }

    pub async fn remove(&self, flatpak_id: &str) {
        let (name, backend, icon_letter, icon_color) = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| (r.name.clone(), r.backend.clone(), r.icon_letter.clone(), r.icon_color_hex.clone()))
                .unwrap_or_else(|| (flatpak_id.to_string(), AppBackend::Flatpak, "?".into(), "#7c3aed".into()))
        };
        let success = match backend {
            AppBackend::Flatpak => flatpak::remove(flatpak_id).await.success,
            AppBackend::Bootc   => bootc::remove(flatpak_id).await.success,
        };
        if success {
            {
                let mut s = self.state.lock().unwrap();
                s.add_recent(flatpak_id, &name, "remove", &icon_letter, &icon_color);
            }
            self.push_recent();
            self.show_notification(format!("{name} removed"), "info");
            self.refresh_installed().await;
            self.window.upgrade_in_event_loop(|w| w.set_detail_visible(false)).ok();
        } else {
            self.show_notification(format!("Failed to remove {name}"), "error");
        }
    }

    pub async fn update_app(&self, flatpak_id: &str) {
        let name: String = {
            let s = self.state.lock().unwrap();
            s.all_apps.iter().find(|a| a.flatpak_id == flatpak_id)
                .map(|r| r.name.clone()).unwrap_or_else(|| flatpak_id.to_string())
        };
        self.show_toast(name.clone().into(), 0.0, "Updating…");
        let result = flatpak::update(flatpak_id).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        self.window.upgrade_in_event_loop(|w| w.set_toast_visible(false)).ok();
        if result.success {
            self.show_notification(format!("{name} updated"), "success");
            self.refresh_installed().await;
        }
    }

    pub async fn update_all(&self) {
        let ids: Vec<String> = { let s = self.state.lock().unwrap(); s.update_ids.clone() };
        let names: Vec<String> = {
            let s = self.state.lock().unwrap();
            ids.iter().filter_map(|id| s.all_apps.iter().find(|a| &a.flatpak_id == id).map(|a| a.name.clone())).collect()
        };
        let ops: Vec<_> = ids.into_iter().zip(names).map(|(fid, name)| (fid, name, "update".to_string())).collect();
        self.run_queue(ops).await;
    }

    // ── Disk usage ────────────────────────────────────────────────────────────

    pub async fn refresh_disk_usage(&self) {
        let (total, reclaimable, breakdown) = flatpak::disk_usage().await;
        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<DiskUsageEntry> = breakdown.into_iter().map(|(label, mb, hex)| {
                DiskUsageEntry { label: label.into(), mb, color: hex_to_color(&hex) }
            }).collect();
            w.set_disk_usage(ModelRc::new(VecModel::from(entries)));
            w.set_disk_total_mb(total);
            w.set_disk_reclaimable(reclaimable);
        }).ok();
    }

    pub async fn run_cleanup(&self) {
        self.show_notification("Running Flatpak cleanup…".to_string(), "info");
        flatpak::cleanup().await;
        self.show_notification("Cleanup complete".to_string(), "success");
        self.refresh_disk_usage().await;
        self.refresh_installed().await;
    }

    // ── Recent activity push ──────────────────────────────────────────────────

    fn push_recent(&self) {
        let recent = { let s = self.state.lock().unwrap(); s.recent.clone() };
        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<RecentEntry> = recent.iter().map(|r| RecentEntry {
                app_id:      r.app_id.clone().into(),
                name:        r.name.clone().into(),
                action:      r.action.clone().into(),
                icon_letter: r.icon_letter.clone().into(),
                icon_color:  hex_to_color(&r.icon_color),
                when:        r.when.clone().into(),
            }).collect();
            w.set_recent_activity(ModelRc::new(VecModel::from(entries)));
        }).ok();
    }

    // ── Notifications ─────────────────────────────────────────────────────────

    fn show_notification(&self, msg: String, notif_type: &'static str) {
        let msg: SharedString = msg.into();
        self.window.upgrade_in_event_loop(move |w| {
            w.set_notif_message(msg);
            w.set_notif_type(notif_type.into());
            w.set_notif_visible(true);
        }).ok();

        // Auto-dismiss after 5s for non-error notifications
        if notif_type != "error" {
            let ww = self.window.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                ww.upgrade_in_event_loop(|w| w.set_notif_visible(false)).ok();
            });
        }
    }

    pub fn dismiss_notification(&self) {
        self.window.upgrade_in_event_loop(|w| w.set_notif_visible(false)).ok();
    }

    // ── UI push ───────────────────────────────────────────────────────────────

    fn push_all(&self) {
        let (slice, total, feat_data, ic, uc, sc, recent) = {
            let s = self.state.lock().unwrap();
            let (slice, total) = s.filtered_sorted_paged();
            let feat  = s.featured();
            let fdata = s.to_app_data(&feat);
            let data  = s.to_app_data(&slice);
            let ic    = s.installed_ids.len() as i32;
            let uc    = s.update_ids.len() as i32;
            let sc    = s.selected_ids.len() as i32;
            let rec   = s.recent.clone();
            (data, total as i32, fdata, ic, uc, sc, rec)
        };

        self.window.upgrade_in_event_loop(move |w| {
            let entries: Vec<AppEntry> = slice.into_iter().map(|d| d.into_entry()).collect();
            w.set_apps(ModelRc::new(VecModel::from(entries)));
            let feat: Vec<AppEntry> = feat_data.into_iter().map(|d| d.into_entry()).collect();
            w.set_featured_apps(ModelRc::new(VecModel::from(feat)));
            w.set_total_count(total);
            w.set_installed_count(ic);
            w.set_updates_count(uc);
            w.set_selected_count(sc);
            w.set_loading(false);
            w.set_loading_more(false);

            let recent_entries: Vec<RecentEntry> = recent.iter().map(|r| RecentEntry {
                app_id:      r.app_id.clone().into(),
                name:        r.name.clone().into(),
                action:      r.action.clone().into(),
                icon_letter: r.icon_letter.clone().into(),
                icon_color:  hex_to_color(&r.icon_color),
                when:        r.when.clone().into(),
            }).collect();
            w.set_recent_activity(ModelRc::new(VecModel::from(recent_entries)));
        }).ok();
    }

    fn show_toast(&self, name: SharedString, progress: f32, status: &'static str) {
        self.window.upgrade_in_event_loop(move |w| {
            w.set_toast_app(name); w.set_toast_progress(progress);
            w.set_toast_status(status.into()); w.set_toast_visible(true);
        }).ok();
    }
}

// ─── Flatpak remotes ─────────────────────────────────────────────────────────

async fn list_flatpak_remotes() -> Vec<(String, String, bool)> {
    let out = tokio::process::Command::new("flatpak")
        .args(["remotes", "--columns=name,url,options"])
        .output().await;
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with("Name"))
                .map(|line| {
                    let cols: Vec<&str> = line.splitn(3, '\t').collect();
                    let name    = cols.first().unwrap_or(&"").trim().to_string();
                    let url     = cols.get(1).unwrap_or(&"").trim().to_string();
                    let options = cols.get(2).unwrap_or(&"").trim().to_lowercase();
                    let enabled = !options.contains("disabled");
                    (name, url, enabled)
                })
                .filter(|(name, _, _)| !name.is_empty())
                .collect()
        }
        _ => vec![("flathub".into(), "https://dl.flathub.org/repo/".into(), true)],
    }
}

// ─── Permissions ─────────────────────────────────────────────────────────────

async fn get_app_permissions(flatpak_id: &str) -> Vec<String> {
    let out = tokio::process::Command::new("flatpak")
        .args(["info", "--show-permissions", flatpak_id])
        .output().await;
    let text = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };
    let mut perms: Vec<String> = vec![];
    for line in text.lines() {
        let l = line.trim().to_lowercase();
        if l.contains("network")                                   { perms.push("Network".into()); }
        if l.contains("home") || l.contains("filesystem")         { perms.push("Filesystem".into()); }
        if l.contains("camera") || l.contains("webcam")           { perms.push("Webcam".into()); }
        if l.contains("microphone") || l.contains("audio-record") { perms.push("Microphone".into()); }
        if l.contains("location")                                  { perms.push("Location".into()); }
        if l.contains("notification")                              { perms.push("Notifications".into()); }
        if l.contains("bluetooth")                                 { perms.push("Bluetooth".into()); }
        if l.contains("usb")                                       { perms.push("USB".into()); }
    }
    perms.dedup();
    perms
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub fn hex_to_color(hex: &str) -> Color {
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
        "internet"     => "Internet",    "multimedia" => "Multimedia",
        "graphics"     => "Graphics",    "productivity" => "Office",
        "development"  => "Dev",         "games"      => "Games",
        "tools"        => "Tools",       "system"     => "System",
        _              => "Other",
    }
}
