# LegendaryOS Store — Roadmap

## ◈ v0.1 — Current release  ✓

**Foundation & core UX**

- Native Wayland window via Slint + softbuffer (no glutin, no OpenGL required)
- Dual backend: **Flatpak** (Flathub) + **bootc** (system packages via dnf5)
- Live catalog from Flathub API v2 — ~250 most popular apps on first load
- System packages queried via `dnf5 repoquery` (fallback to seed list)
- Fuzzy multi-field search: name → flatpak ID → developer → summary → description → tags
  - Relevance scoring with token matching
  - "X results for …" counter
  - Clear button, placeholder text
- Category sidebar: Internet / Multimedia / Graphics / Office / Dev / Games / Tools / System
- Installed / Updates library sections with count badges
- App card grid (4-per-row, scrollable, up to 252 apps rendered)
  - Installed green dot indicator
  - bootc / Flatpak badge
  - Install / Remove animated button with state
- Slide-in detail panel (330px, animated)
  - Install status banner (green when installed)
  - Version / size / installs stats
  - Full description
  - Reboot warning for bootc packages
- Progress toast (bottom-right overlay, animated)
- pkexec polkit auth for system packages

---

## ◈ v0.2 — Search & Discovery  ✓

**Better finding, better browsing**

- [x] Pagination — PAGE=40 with Load More button, up to 500 apps from Flathub API
- [x] Autocomplete dropdown with app name suggestions + recent search history
- [x] Search history (last 8 queries shown in suggestions)
- [x] Sort pills: Popular / Name A-Z / Rating / Size / Recent
- [ ] "Similar apps" section in detail panel
- [x] Featured banner — top 3 highest-rated apps shown on home screen
- [ ] Recently installed section in sidebar  (v0.3)
- [ ] Keyboard navigation  (v0.3)

---

## ◈ v0.3 — App details & rich catalog  ✓

**Catalog fix:**
- [x] Primary source: `flatpak remote-ls flathub` (same as GNOME Software) — works offline
- [x] HTTP fallback: Flathub API v2 with browser User-Agent
- [x] Always-working seed list (30 Flatpak + 18 bootc packages)
- [x] `guess_category_from_id()` — smart category detection from reverse-DNS app ID
- [x] `extract_developer_from_id()` — developer name from app ID domain
- [x] Pseudo-rating from app ID hash (deterministic) until real ratings API


**Rich app information**

- [x] Screenshot strip component (placeholder, image fetch in v0.4)
- [x] Screenshot carousel layout with slot placeholders
- [x] Changelog/permissions panel components
- [x] Permissions chips with colour-coded icons (Network/FS/Camera/Mic/Location…)
- [ ] Website / issue links  (v0.4)
- [ ] Size breakdown  (v0.4)
- [ ] Version history  (v0.4)
- [ ] Rating distribution  (v0.4)

---

## v0.4 — Installation management  🔜

**Control over what's installed**

- [ ] Batch operations: select multiple apps → install/remove all
- [ ] "Update all" button in Updates section
- [ ] Installation queue — show pending operations in sidebar badge
- [ ] Pause / cancel in-progress installations
- [ ] Auto-update scheduler (daily background check)
- [ ] Disk usage summary: "You have N apps using X GB"
- [ ] Orphaned data cleanup (leftover Flatpak data / cache)
- [ ] Flatpak remote management (add / remove / enable / disable remotes)

---

## v0.5 — bootc deep integration  🔜

**First-class immutable OS support**

- [ ] Live `bootc status` panel: current image tag, staged deployment, last update
- [ ] "System upgrade" button: `bootc upgrade` with progress + reboot prompt
- [ ] Containerfile editor: compose custom image layers (advanced)
- [ ] Reboot-pending banner when staged changes exist
- [ ] Show which packages are in base image vs layered
- [ ] One-click reboot-to-apply after system package install
- [ ] Rollback to previous deployment via `bootc rollback`
- [ ] Overlay diff: see exactly what's added vs the base image

---

## v0.6 — Personalisation & collections  🔜

**Make it yours**

- [ ] Collections / lists: save curated app sets ("Gaming rig", "Dev workstation")
- [ ] Import / export collections as JSON (share with others)
- [ ] Wishlist: mark apps to install later
- [ ] Starred / favourite apps (persist across sessions via XDG config)
- [ ] Custom categories / tags
- [ ] Per-app notes (local text attached to an app entry)
- [ ] Theme selector: Dark (default), Darker, Light, High-contrast

---

## v0.7 — Notifications & background service  🔜

**System integration**

- [ ] Background daemon (systemd user service) for update polling
- [ ] Desktop notifications via `notify-send` / D-Bus when updates available
- [ ] System tray icon (update count badge)
- [ ] GNOME Shell / KDE Plasma extension integration
- [ ] Post-install ".desktop autostart" prompt for relevant apps
- [ ] Flatpak portal integration (file picker, network, location dialogs)
- [ ] D-Bus API so other tools can query/trigger installs

---

## v0.8 — Polish & performance  🔜

**Production-grade release**

- [ ] Icon download & disk cache: real app icons from Flathub CDN
- [ ] Full AppStream XML parser (offline metadata, no API dependency)
- [ ] Local metadata cache with incremental updates (like PackageKit)
- [ ] 60 fps animations everywhere (Slint Skia-Vulkan renderer optional)
- [ ] Accessibility: screen reader support (ATK / AT-SPI), keyboard-only flow
- [ ] Localisation / i18n framework (gettext, Polish + English + more)
- [ ] Crash reporting (optional, opt-in)
- [ ] Flatpak packaging of LegendaryOS Store itself
- [ ] CI pipeline: cargo test + cargo clippy + slint-check on every commit
- [ ] Man page + `--help` CLI flags (headless install mode)
- [ ] GNOME Software / PackageKit plugin compatibility shim

---

## Beyond v0.8

- OCI image browser (pull and inspect bootc images)
- Flathub user reviews & ratings submission
- Enterprise repo support (private Flatpak remotes, air-gap mode)
- Wayland kiosk mode (for LegendaryOS installer live environment)
