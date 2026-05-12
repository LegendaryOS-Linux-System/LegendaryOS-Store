use crate::app_data::{AppBackend, AppRecord};
use tokio::process::Command;

// ─── Categories ───────────────────────────────────────────────────────────────

pub fn categories() -> Vec<(String, String, String)> {
    vec![
        ("all".into(),          "All Apps".into(),     "◈".into()),
        ("internet".into(),     "Internet".into(),      "⊕".into()),
        ("multimedia".into(),   "Multimedia".into(),    "▶".into()),
        ("graphics".into(),     "Graphics".into(),      "◉".into()),
        ("productivity".into(), "Productivity".into(),  "☰".into()),
        ("development".into(),  "Development".into(),   "⌘".into()),
        ("games".into(),        "Games".into(),         "◈".into()),
        ("tools".into(),        "Tools".into(),         "⊞".into()),
        ("system".into(),       "System".into(),        "◎".into()),
    ]
}

// ─── Flathub ──────────────────────────────────────────────────────────────────

/// Minimal subset of what Flathub API returns per app.
#[derive(serde::Deserialize, Debug)]
struct FlathubApp {
    #[serde(rename = "id")]
    app_id: String,
    name: String,
    summary: Option<String>,
    #[serde(rename = "iconDesktopUrl")]
    icon_desktop_url: Option<String>,
}

/// Fetch apps from Flathub v2 API, map into AppRecord.
/// Falls back to a small seed list if network is unavailable.
pub async fn fetch_flatpak_apps() -> Vec<AppRecord> {
    match try_fetch_flathub().await {
        Ok(apps) if !apps.is_empty() => apps,
        _ => {
            eprintln!("[catalog] Flathub API unreachable — using seed list");
            flatpak_seed()
        }
    }
}

async fn try_fetch_flathub() -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("LegendaryOS-Store/0.1")
        .build()?;

    // Flathub summary endpoint — returns all apps (~2000 entries), paginated
    // We fetch the first page (limit 250) sorted by installs for "popular" feel
    let url = "https://flathub.org/api/v2/apps?page=1&per_page=250&sort=installs";
    let resp = client.get(url).send().await?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }

    // The v2 API returns { hits: [...], total: N }
    let raw: serde_json::Value = resp.json().await?;
    let hits = raw["hits"].as_array()
        .ok_or("missing hits array")?;

    let records: Vec<AppRecord> = hits.iter().filter_map(|v| {
        let app_id = v["app_id"].as_str().or(v["id"].as_str())?;
        let name = v["name"].as_str().unwrap_or(app_id);
        let summary = v["summary"].as_str().unwrap_or("").to_string();
        let description = v["description"].as_str()
            .unwrap_or(&summary).to_string();
        let developer = v["developer_name"].as_str()
            .or(v["project_group"].as_str())
            .unwrap_or("Unknown").to_string();
        let version = v["version"].as_str().unwrap_or("latest").to_string();

        // Map Flathub category string to our internal category id
        let raw_cat = v["categories"].as_array()
            .and_then(|a| a.first())
            .and_then(|c| c.as_str())
            .unwrap_or("other");
        let category = map_flathub_category(raw_cat);

        // Download / install count (Flathub uses "installs" field)
        let dl = v["installs"].as_u64()
            .map(format_count)
            .unwrap_or_else(|| "N/A".into());

        // Icon: first letter of name
        let icon_letter = name.chars().next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into());

        let icon_color = category_color(category);
        let rating = v["rating"].as_f64().map(|r| r as f32).unwrap_or(4.0);

        Some(AppRecord {
            id:             app_id.replace('.', "-").to_lowercase(),
            flatpak_id:     app_id.to_string(),
            name:           name.to_string(),
            summary:        if summary.len() > 120 { summary[..120].to_string() + "…" } else { summary },
            description,
            version,
            developer,
            category:       category.to_string(),
            icon_letter,
            icon_color_hex: icon_color.to_string(),
            rating,
            download_count: dl,
            size_mb:        0.0,   // not in summary, would need detail call
            backend:        AppBackend::Flatpak,
        })
    }).collect();

    eprintln!("[catalog] Flathub: loaded {} apps", records.len());
    Ok(records)
}

fn map_flathub_category(raw: &str) -> &'static str {
    match raw.to_lowercase().as_str() {
        "webbrowser" | "network" | "chat" | "email" => "internet",
        "video" | "audio" | "audiovideo" | "music"  => "multimedia",
        "graphics" | "photography"                   => "graphics",
        "office" | "productivity" | "finance"        => "productivity",
        "development" | "ide" | "debugger"           => "development",
        "game" | "games" | "emulator"                => "games",
        "utility" | "utilities" | "system" | "settings" => "tools",
        _ => "tools",
    }
}

fn category_color(cat: &str) -> &'static str {
    match cat {
        "internet"     => "#3b82f6",
        "multimedia"   => "#e8861a",
        "graphics"     => "#5c8926",
        "productivity" => "#18a303",
        "development"  => "#007acc",
        "games"        => "#9333ea",
        "tools"        => "#d946ef",
        "system"       => "#6b7280",
        _              => "#7c3aed",
    }
}

fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ─── Seed fallback (shown while API loads or if offline) ─────────────────────

fn flatpak_seed() -> Vec<AppRecord> {
    vec![
        mk_flatpak("org.mozilla.firefox",           "Firefox",       "Fast, private & secure browser",           "internet",     "#e25c00", "F", 4.7, "2.1M"),
        mk_flatpak("org.videolan.VLC",              "VLC",           "The ultimate media player",                 "multimedia",   "#e8861a", "V", 4.8, "1.8M"),
        mk_flatpak("com.visualstudio.code",         "VS Code",       "Powerful code editor by Microsoft",         "development",  "#007acc", "C", 4.9, "3.2M"),
        mk_flatpak("org.gimp.GIMP",                 "GIMP",          "Professional image editing",                "graphics",     "#5c8926", "G", 4.5, "980K"),
        mk_flatpak("com.obsproject.Studio",         "OBS Studio",    "Free streaming & recording",                "multimedia",   "#6441a5", "O", 4.8, "1.2M"),
        mk_flatpak("org.libreoffice.LibreOffice",   "LibreOffice",   "Complete office suite",                     "productivity", "#18a303", "L", 4.4, "1.5M"),
        mk_flatpak("org.inkscape.Inkscape",         "Inkscape",      "Professional vector graphics editor",       "graphics",     "#000000", "I", 4.6, "620K"),
        mk_flatpak("org.signal.Signal",             "Signal",        "Private, encrypted messenger",              "internet",     "#3a76f0", "S", 4.7, "890K"),
        mk_flatpak("org.blender.Blender",           "Blender",       "3D creation: model, rig, render",           "graphics",     "#ea7600", "B", 4.9, "1.1M"),
        mk_flatpak("com.valvesoftware.Steam",       "Steam",         "The ultimate gaming platform",              "games",        "#1b2838", "S", 4.6, "4.5M"),
        mk_flatpak("org.kde.kdenlive",              "Kdenlive",      "Non-linear video editor",                   "multimedia",   "#2196f3", "K", 4.3, "450K"),
        mk_flatpak("org.mozilla.Thunderbird",       "Thunderbird",   "Email, calendar, and chat client",          "internet",     "#0a84ff", "T", 4.3, "760K"),
        mk_flatpak("com.usebottles.bottles",        "Bottles",       "Run Windows software on Linux",             "tools",        "#9b59b6", "B", 4.5, "530K"),
        mk_flatpak("com.github.tchx84.Flatseal",   "Flatseal",      "Manage Flatpak permissions",                "tools",        "#4caf50", "F", 4.7, "310K"),
        mk_flatpak("org.kde.krita",                 "Krita",         "Digital painting and illustration",         "graphics",     "#3daee9", "K", 4.8, "870K"),
        mk_flatpak("io.github.celluloid_player.Celluloid", "Celluloid", "Simple mpv video player",               "multimedia",   "#e91e63", "C", 4.4, "200K"),
        mk_flatpak("com.helix_editor.Helix",        "Helix",         "Post-modern modal text editor",             "development",  "#d946ef", "H", 4.8, "180K"),
        mk_flatpak("org.gnome.Boxes",               "Boxes",         "Virtual machine manager",                   "tools",        "#e01b24", "B", 4.3, "290K"),
        mk_flatpak("net.ankiweb.Anki",              "Anki",          "Powerful spaced repetition flashcards",     "productivity", "#0095da", "A", 4.7, "410K"),
        mk_flatpak("io.podman_desktop.PodmanDesktop","Podman Desktop","Container and Kubernetes manager",         "development",  "#892ca0", "P", 4.5, "150K"),
    ]
}

fn mk_flatpak(id: &str, name: &str, summary: &str, cat: &str,
              color: &str, letter: &str, rating: f32, dl: &str) -> AppRecord {
    AppRecord {
        id:             id.replace('.', "-").to_lowercase(),
        flatpak_id:     id.into(),
        name:           name.into(),
        summary:        summary.into(),
        description:    format!("{name} is available from Flathub as a sandboxed Flatpak application."),
        version:        "latest".into(),
        developer:      "".into(),
        category:       cat.into(),
        icon_letter:    letter.into(),
        icon_color_hex: color.into(),
        rating,
        download_count: dl.into(),
        size_mb:        0.0,
        backend:        AppBackend::Flatpak,
    }
}

// ─── bootc packages ───────────────────────────────────────────────────────────

/// Query available system packages from the Fedora/LegendaryOS rpm-md repos
/// using `dnf5 repoquery` — read-only, no root needed.
/// Falls back to a curated seed list if dnf5 isn't available.
pub async fn fetch_bootc_packages() -> Vec<AppRecord> {
    match try_dnf5_repoquery().await {
        Ok(pkgs) if !pkgs.is_empty() => pkgs,
        Err(e) => {
            eprintln!("[catalog] dnf5 repoquery failed ({e}), using bootc seed");
            bootc_seed()
        }
        _ => bootc_seed(),
    }
}

/// Run `dnf5 repoquery --qf '%{name}|%{summary}|%{version}|%{group}|%{size}'`
/// for a curated set of categories that make sense to install via bootc.
async fn try_dnf5_repoquery() -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>> {
    // We query a fixed set of package names / globs that are sensible
    // system-level additions on an immutable Fedora system.
    let groups = [
        ("multimedia", &["ffmpeg", "gstreamer1-plugins-*", "libavcodec*", "vlc"][..]),
        ("development", &["gcc", "clang", "rustup", "golang", "nodejs", "python3-pip", "docker-ce", "podman"]),
        ("tools", &["fish", "zsh", "htop", "neofetch", "btop", "tmux", "git", "curl", "wget"]),
        ("system", &["wireguard-tools", "openssl", "kernel-headers", "dkms"]),
    ];

    let mut all: Vec<AppRecord> = Vec::new();

    for (category, packages) in &groups {
        let pkg_list = packages.join(" ");
        let out = Command::new("dnf5")
            .args([
                "repoquery",
                "--quiet",
                "--qf", "%{name}|%{summary}|%{version}|%{arch}",
            ])
            .args(packages.iter())
            .output()
            .await?;

        if !out.status.success() {
            continue;
        }

        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() < 3 { continue; }
            let pkg_name = parts[0].trim();
            let summary  = parts[1].trim();
            let version  = parts[2].trim();
            if pkg_name.is_empty() { continue; }

            let icon_letter = pkg_name.chars().next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "P".into());

            all.push(AppRecord {
                id:             format!("bootc-{}", pkg_name),
                flatpak_id:     pkg_name.to_string(),
                name:           pkg_name.to_string(),
                summary:        summary.to_string(),
                description:    format!("{summary}\n\nInstalled at system level via bootc. A reboot is required after installation."),
                version:        version.to_string(),
                developer:      "Fedora Project".into(),
                category:       category.to_string(),
                icon_letter,
                icon_color_hex: "#16a34a".into(),
                rating:         4.0,
                download_count: "N/A".into(),
                size_mb:        0.0,
                backend:        AppBackend::Bootc,
            });
        }
        drop(pkg_list); // suppress unused warning
    }

    eprintln!("[catalog] dnf5: loaded {} system packages", all.len());
    Ok(all)
}

fn bootc_seed() -> Vec<AppRecord> {
    vec![
        mk_bootc("ffmpeg",            "FFmpeg",          "Complete multimedia framework & codec suite",          "multimedia",   "#00a651"),
        mk_bootc("fish",              "Fish Shell",      "Friendly interactive command shell",                   "tools",        "#4bb5c1"),
        mk_bootc("neovim",            "Neovim",          "Hyperextensible Vim-based text editor",                "development",  "#57a143"),
        mk_bootc("docker-ce",         "Docker",          "Container engine for development & production",        "development",  "#0db7ed"),
        mk_bootc("podman",            "Podman",          "Daemonless container engine",                          "development",  "#892ca0"),
        mk_bootc("wireguard-tools",   "WireGuard",       "Fast, modern VPN with state-of-the-art cryptography", "system",       "#88171a"),
        mk_bootc("btop",              "btop",            "Resource monitor — CPU, memory, disks, network",       "tools",        "#f97316"),
        mk_bootc("tmux",              "tmux",            "Terminal multiplexer: split panes, sessions, windows", "tools",        "#22c55e"),
        mk_bootc("zsh",               "Zsh",             "Extended Bourne shell with many improvements",         "tools",        "#4ade80"),
        mk_bootc("gcc",               "GCC",             "GNU Compiler Collection — C, C++, Fortran",            "development",  "#f59e0b"),
        mk_bootc("clang",             "Clang",           "LLVM C/C++/ObjC compiler with great diagnostics",      "development",  "#a78bfa"),
        mk_bootc("golang",            "Go",              "Fast, statically typed compiled language by Google",   "development",  "#00acd7"),
        mk_bootc("nodejs",            "Node.js",         "JavaScript runtime built on Chrome's V8 engine",       "development",  "#5fa04e"),
        mk_bootc("kernel-headers",    "Kernel Headers",  "Header files for the Linux kernel (needed for DKMS)",  "system",       "#6b7280"),
        mk_bootc("akmod-nvidia",      "NVIDIA Drivers",  "Proprietary NVIDIA GPU drivers via akmods",            "system",       "#76b900"),
        mk_bootc("openssl",           "OpenSSL",         "Robust TLS/SSL and general purpose cryptography",      "system",       "#e11d48"),
    ]
}

fn mk_bootc(pkg: &str, name: &str, summary: &str, cat: &str, color: &str) -> AppRecord {
    let icon_letter = name.chars().next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "P".into());
    AppRecord {
        id:             format!("bootc-{pkg}"),
        flatpak_id:     pkg.into(),
        name:           name.into(),
        summary:        summary.into(),
        description:    format!("{summary}\n\nThis is a system-level package installed via bootc. Changes take effect after reboot."),
        version:        "latest".into(),
        developer:      "Fedora Project".into(),
        category:       cat.into(),
        icon_letter,
        icon_color_hex: color.into(),
        rating:         4.2,
        download_count: "N/A".into(),
        size_mb:        0.0,
        backend:        AppBackend::Bootc,
    }
}
