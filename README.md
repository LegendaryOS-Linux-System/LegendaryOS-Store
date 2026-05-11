# ◈ LegendaryOS Store

> The legendary way to manage applications on LegendaryOS.

A native Flatpak software center built in **Rust + Slint**, designed exclusively for
LegendaryOS — the immutable Fedora-based distribution using `bootc` and Flatpak
as the primary user-space app delivery mechanism.

---

## Screenshot

Dark retrowave aesthetic with magenta/violet palette inspired by the LegendaryOS phoenix logo.
Full sidebar navigation, app grid, detail panel, and install progress toast.

---

## Features

| Feature | Details |
|---|---|
| 🔍 **Search** | Real-time filtering across name, summary, developer, category |
| 📁 **Categories** | Internet, Multimedia, Graphics, Office, Development, Games, Tools |
| ⊕ **Install** | One-click Flatpak install from Flathub with live progress toast |
| ⊗ **Remove** | Uninstall apps with confirmation |
| ⊞ **Installed** | View only your installed apps |
| ⟳ **Updates** | See which apps have updates available |
| ◈ **Detail panel** | Slide-in panel with full metadata, description, flatpak ID |
| 🎨 **UI** | Retrowave dark theme — black, magenta `#d946ef`, violet `#7c3aed`, blue `#3b82f6` |

---

## Architecture

```
legendary-store/
├── Cargo.toml
├── build.rs                  # compiles Slint UI
├── ui/
│   └── store.slint           # All UI components (single file)
└── src/
    ├── main.rs               # Entry point, callback wiring
    ├── app_data.rs           # Static catalog + category definitions
    ├── flatpak.rs            # Async flatpak CLI integration
    └── store_model.rs        # State management + Slint bridge
```

### Technology stack

- **[Rust](https://www.rust-lang.org/)** — safe, fast systems language
- **[Slint](https://slint.dev/)** — native UI toolkit with `.slint` DSL (GPU-accelerated, no Electron)
- **[Tokio](https://tokio.rs/)** — async runtime for non-blocking flatpak subprocess calls
- **[Flatpak CLI](https://flatpak.org/)** — app install/remove/update via `flatpak` subprocess

---

## Requirements

- LegendaryOS (Fedora + bootc) or any Fedora-based system
- `flatpak` installed and `flathub` remote configured
- Rust toolchain (`rustup`) ≥ 1.77
- GPU with OpenGL/Vulkan for Slint rendering

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Add Flathub remote (if not already)

```bash
flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
```

---

## Build & Run

```bash
# Clone / enter the project
cd legendary-store

# Debug build + run
cargo run

# Optimised release build
cargo build --release
./target/release/legendary-store
```

---

## Slint UI structure (`ui/store.slint`)

| Component | Purpose |
|---|---|
| `MainWindow` | Root window, holds all state properties |
| `TopBar` | Header with logo, installed count, updates badge |
| `SidebarItem` | Individual nav item with active indicator |
| `AppCard` | Grid card: icon, name, rating, badges, action button |
| `DetailPanel` | Slide-in right panel with full app info |
| `SearchBar` | Styled text input with live clear button |
| `StarRating` | Pixel-style 5-star rating row |
| `Badge` | Glowing pill label (category, size) |
| `AppIcon` | Letter-based icon placeholder with glow |
| `ProgressToast` | Floating install progress overlay |

---

## Extending the catalog

Edit `src/app_data.rs` → `builtin_catalog()` to add more apps.  
Each `AppRecord` needs: `flatpak_id`, `name`, `category`, `icon_letter`, `icon_color_hex`.

Future: pull live metadata from the [Flathub API](https://flathub.org/api/v2/apps).

---

## Color palette

| Token | Hex | Usage |
|---|---|---|
| Background | `#080310` | Window background |
| Surface | `#0a0412` | Sidebar, topbar |
| Card | `#120820` | App cards |
| Border | `#2a1040` | Dividers, inactive borders |
| Primary | `#7c3aed` | Violet — primary accent |
| Hot | `#d946ef` | Magenta — CTAs, glows |
| Blue | `#3b82f6` | Info badges, flatpak icon |
| Text | `#f0e6ff` | Primary text |
| Muted | `#9980c4` | Secondary text |
| Dim | `#6b4a9e` | Tertiary / metadata |

---

## License

GPL 3.0 © LegendaryOS Team
