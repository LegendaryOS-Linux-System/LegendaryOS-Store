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

// ─── Flathub v2 API ───────────────────────────────────────────────────────────

pub async fn fetch_flatpak_apps() -> Vec<AppRecord> {
    match try_fetch_all_pages().await {
        Ok(apps) if !apps.is_empty() => {
            eprintln!("[catalog] Flathub: {} apps fetched", apps.len());
            apps
        }
        Err(e) => {
            eprintln!("[catalog] Flathub API error: {e} — using seed");
            flatpak_seed()
        }
        _ => {
            eprintln!("[catalog] Flathub returned empty — using seed");
            flatpak_seed()
        }
    }
}

/// Fetch up to 500 apps across multiple API pages (250/page).
async fn try_fetch_all_pages()
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("LegendaryOS-Store/0.2 (https://github.com/LegendaryOS)")
        .build()?;

    let mut all: Vec<AppRecord> = vec![];

    for page in 1..=2u32 {    // 2 pages × 250 = up to 500 apps
        let url = format!(
            "https://flathub.org/api/v2/apps?page={page}&per_page=250&sort=installs"
        );
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[catalog] page {page} error: {e}");
                break;
            }
        };
        if !resp.status().is_success() {
            eprintln!("[catalog] page {page} HTTP {}", resp.status());
            break;
        }

        let raw: serde_json::Value = resp.json().await?;

        // Flathub v2: { "hits": [...], "total": N }
        let hits = match raw["hits"].as_array() {
            Some(a) => a,
            None    => break,
        };
        if hits.is_empty() { break; }

        let records: Vec<AppRecord> = hits.iter().filter_map(|v| parse_flathub_app(v)).collect();
        eprintln!("[catalog] page {page}: {} apps", records.len());
        all.extend(records);

        // Avoid rate limiting
        if page < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }

    Ok(all)
}

fn parse_flathub_app(v: &serde_json::Value) -> Option<AppRecord> {
    let app_id  = v["app_id"].as_str().or_else(|| v["id"].as_str())?;
    let name    = v["name"].as_str().unwrap_or(app_id);
    let summary = v["summary"].as_str().unwrap_or("").to_string();
    let desc    = v["description"].as_str().unwrap_or(&summary).to_string();
    let dev     = v["developer_name"].as_str()
                    .or_else(|| v["project_group"].as_str())
                    .unwrap_or("").to_string();
    let version = v["version"].as_str().unwrap_or("").to_string();

    let raw_cat = v["categories"].as_array()
        .and_then(|a| a.first())
        .and_then(|c| c.as_str())
        .unwrap_or("other");
    let category = map_flathub_category(raw_cat).to_string();

    let dl = v["installs"].as_u64().map(fmt_count).unwrap_or_else(|| "N/A".into());
    let rating = v["rating"].as_f64().map(|r| r as f32)
                   .unwrap_or_else(|| 3.5 + (app_id.len() % 3) as f32 * 0.3);
    let rating = rating.min(5.0).max(1.0);

    let icon_letter = name.chars().next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    let icon_color = category_color(&category).to_string();

    let icon_url = v["icon"].as_str()
        .or_else(|| v["iconDesktopUrl"].as_str())
        .unwrap_or("").to_string();

    Some(AppRecord {
        id:             app_id.replace('.', "-").to_lowercase(),
        flatpak_id:     app_id.to_string(),
        name:           name.to_string(),
        summary:        truncate(&summary, 120),
        description:    truncate(&desc, 600),
        version:        if version.is_empty() { "latest".into() } else { version },
        developer:      dev,
        category,
        icon_letter,
        icon_color_hex: icon_color,
        icon_url,
        rating,
        download_count: dl,
        size_mb:        0.0,
        backend:        AppBackend::Flatpak,
    })
}

fn map_flathub_category(raw: &str) -> &'static str {
    match raw.to_lowercase().as_str() {
        "webbrowser"|"network"|"chat"|"email"|"feed"       => "internet",
        "video"|"audio"|"audiovideo"|"music"|"player"      => "multimedia",
        "graphics"|"photography"|"2dgraphics"|"3dgraphics" => "graphics",
        "office"|"productivity"|"finance"|"calculator"     => "productivity",
        "development"|"ide"|"debugger"|"building"|"vcs"    => "development",
        "game"|"games"|"emulator"|"boardgame"|"actiongame" => "games",
        "utility"|"utilities"|"settings"|"accessibility"   => "tools",
        "system"|"monitor"|"security"|"terminalemulator"   => "system",
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
        "system"       => "#6b7280",
        _              => "#d946ef",
    }
}

fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
    else if n >= 1_000 { format!("{:.0}K", n as f64 / 1_000.0) }
    else { n.to_string() }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else { s[..max].to_string() + "…" }
}

// ─── bootc / dnf5 repoquery ───────────────────────────────────────────────────

pub async fn fetch_bootc_packages() -> Vec<AppRecord> {
    match try_dnf5_repoquery().await {
        Ok(v) if !v.is_empty() => {
            eprintln!("[catalog] dnf5: {} system packages", v.len());
            v
        }
        Err(e) => {
            eprintln!("[catalog] dnf5 error: {e} — using bootc seed");
            bootc_seed()
        }
        _ => bootc_seed(),
    }
}

async fn try_dnf5_repoquery()
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    // Curated groups of packages meaningful to install on immutable Fedora
    let queries: &[(&str, &[&str])] = &[
        ("multimedia",   &["ffmpeg","gstreamer1-plugins-bad-free","gstreamer1-plugins-ugly","vlc",
                           "mkvtoolnix","mediainfo","lame","opus-tools"]),
        ("development",  &["gcc","clang","make","cmake","git","neovim","helix","golang",
                           "nodejs","python3-pip","rustup","podman","buildah","skopeo"]),
        ("tools",        &["fish","zsh","htop","btop","neofetch","tmux","screen",
                           "curl","wget","rsync","p7zip","unrar"]),
        ("system",       &["wireguard-tools","openssl","kernel-headers","dkms",
                           "fuse","fuse3","ntfs-3g","exfatprogs"]),
        ("internet",     &["thunderbird","neomutt","irssi","weechat"]),
        ("games",        &["lutris","wine","winetricks","gamemode","mangohud"]),
    ];

    let mut all: Vec<AppRecord> = vec![];

    for (category, pkgs) in queries {
        let out = Command::new("dnf5")
            .args(["repoquery", "--quiet", "--qf", "%{name}|%{summary}|%{version}|%{size}"])
            .args(pkgs.iter())
            .output()
            .await;

        let text = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => continue,
        };

        for line in text.lines() {
            let p: Vec<&str> = line.splitn(4, '|').collect();
            if p.len() < 3 || p[0].is_empty() { continue; }
            let pkg_name = p[0].trim();
            let summary  = p[1].trim();
            let version  = p[2].trim();
            let size_mb  = p.get(3)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .map(|b| b / 1_048_576.0)
                .unwrap_or(0.0) as f32;

            all.push(AppRecord {
                id:             format!("bootc-{pkg_name}"),
                flatpak_id:     pkg_name.to_string(),
                name:           pkg_name.to_string(),
                summary:        summary.to_string(),
                description:    format!("{summary}\n\nSystem package installed via bootc (dnf5). Reboot required after install."),
                version:        version.to_string(),
                developer:      "Fedora Project".into(),
                category:       category.to_string(),
                icon_letter:    pkg_name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "P".into()),
                icon_color_hex: "#16a34a".into(),
                rating:         4.0,
                download_count: "N/A".into(),
                size_mb,
                icon_url:       "".into(),
                backend:        AppBackend::Bootc,
            });
        }
    }

    Ok(all)
}

// ─── Seed fallbacks ───────────────────────────────────────────────────────────

fn flatpak_seed() -> Vec<AppRecord> {
    vec![
        mkfp("org.mozilla.firefox",          "Firefox",       "Fast, private & secure browser",      "internet",     "#e25c00","F",4.7,"2.1M"),
        mkfp("org.videolan.VLC",             "VLC",           "The ultimate media player",            "multimedia",   "#e8861a","V",4.8,"1.8M"),
        mkfp("com.visualstudio.code",        "VS Code",       "Powerful code editor by Microsoft",   "development",  "#007acc","C",4.9,"3.2M"),
        mkfp("org.gimp.GIMP",                "GIMP",          "Professional image editing",          "graphics",     "#5c8926","G",4.5,"980K"),
        mkfp("com.obsproject.Studio",        "OBS Studio",    "Free streaming & recording",          "multimedia",   "#6441a5","O",4.8,"1.2M"),
        mkfp("org.libreoffice.LibreOffice",  "LibreOffice",   "Complete office suite",               "productivity", "#18a303","L",4.4,"1.5M"),
        mkfp("org.inkscape.Inkscape",        "Inkscape",      "Professional vector graphics",        "graphics",     "#000000","I",4.6,"620K"),
        mkfp("org.signal.Signal",            "Signal",        "Private encrypted messenger",         "internet",     "#3a76f0","S",4.7,"890K"),
        mkfp("org.blender.Blender",          "Blender",       "3D creation suite",                   "graphics",     "#ea7600","B",4.9,"1.1M"),
        mkfp("com.valvesoftware.Steam",      "Steam",         "The ultimate gaming platform",        "games",        "#1b2838","S",4.6,"4.5M"),
        mkfp("org.kde.kdenlive",             "Kdenlive",      "Non-linear video editor",             "multimedia",   "#2196f3","K",4.3,"450K"),
        mkfp("org.mozilla.Thunderbird",      "Thunderbird",   "Email, calendar and chat",            "internet",     "#0a84ff","T",4.3,"760K"),
        mkfp("com.usebottles.bottles",       "Bottles",       "Run Windows software on Linux",       "tools",        "#9b59b6","B",4.5,"530K"),
        mkfp("com.github.tchx84.Flatseal",  "Flatseal",      "Manage Flatpak permissions",          "tools",        "#4caf50","F",4.7,"310K"),
        mkfp("org.kde.krita",                "Krita",         "Digital painting & illustration",     "graphics",     "#3daee9","K",4.8,"870K"),
        mkfp("com.spotify.Client",           "Spotify",       "Music streaming",                     "multimedia",   "#1db954","S",4.5,"2.4M"),
        mkfp("org.gnome.Boxes",              "Boxes",         "Virtual machine manager",             "tools",        "#e01b24","B",4.3,"290K"),
        mkfp("net.ankiweb.Anki",             "Anki",          "Spaced repetition flashcards",        "productivity", "#0095da","A",4.7,"410K"),
        mkfp("com.discordapp.Discord",       "Discord",       "Voice, video and text chat",          "internet",     "#5865f2","D",4.4,"3.1M"),
        mkfp("org.telegram.desktop",         "Telegram",      "Fast and secure messenger",           "internet",     "#2ca5e0","T",4.6,"2.8M"),
        mkfp("io.podman_desktop.PodmanDesktop","Podman Desktop","Container and Kubernetes manager",  "development",  "#892ca0","P",4.5,"150K"),
        mkfp("com.github.PintaProject.Pinta","Pinta",         "Simple drawing and editing",          "graphics",     "#e05c1e","P",4.1,"180K"),
        mkfp("org.audacityteam.Audacity",    "Audacity",      "Free audio editor",                   "multimedia",   "#0080c0","A",4.3,"650K"),
        mkfp("org.darktable.Darktable",      "Darktable",     "Professional photo workflow",         "graphics",     "#3b3b3b","D",4.5,"340K"),
    ]
}

fn mkfp(id: &str, name: &str, summary: &str, cat: &str,
        color: &str, letter: &str, rating: f32, dl: &str) -> AppRecord {
    AppRecord {
        id:             id.replace('.', "-").to_lowercase(),
        flatpak_id:     id.into(),
        name:           name.into(),
        summary:        summary.into(),
        description:    format!("{name} is available from Flathub as a sandboxed Flatpak application.\n\n{summary}"),
        version:        "latest".into(),
        developer:      "".into(),
        category:       cat.into(),
        icon_letter:    letter.into(),
        icon_color_hex: color.into(),
        rating,
        download_count: dl.into(),
        size_mb:        0.0,
        icon_url:       "".into(),
        backend:        AppBackend::Flatpak,
    }
}

fn bootc_seed() -> Vec<AppRecord> {
    vec![
        mkbc("ffmpeg",          "FFmpeg",          "Complete multimedia codec framework",          "multimedia",   "#00a651"),
        mkbc("fish",            "Fish Shell",      "Friendly interactive shell",                   "tools",        "#4bb5c1"),
        mkbc("neovim",          "Neovim",          "Hyperextensible Vim-based text editor",        "development",  "#57a143"),
        mkbc("podman",          "Podman",          "Daemonless OCI container engine",              "development",  "#892ca0"),
        mkbc("wireguard-tools", "WireGuard",       "Fast, modern VPN",                            "system",       "#88171a"),
        mkbc("btop",            "btop",            "Resource monitor — CPU, RAM, disk, net",       "tools",        "#f97316"),
        mkbc("tmux",            "tmux",            "Terminal multiplexer",                         "tools",        "#22c55e"),
        mkbc("gcc",             "GCC",             "GNU Compiler Collection",                      "development",  "#f59e0b"),
        mkbc("clang",           "Clang",           "LLVM C/C++ compiler",                         "development",  "#a78bfa"),
        mkbc("golang",          "Go",              "Statically typed compiled language",           "development",  "#00acd7"),
        mkbc("nodejs",          "Node.js",         "JavaScript runtime (V8)",                      "development",  "#5fa04e"),
        mkbc("kernel-headers",  "Kernel Headers",  "Linux kernel headers (needed for DKMS)",       "system",       "#6b7280"),
        mkbc("wireguard-tools", "WireGuard Tools", "WireGuard VPN management tools",               "system",       "#88171a"),
        mkbc("lutris",          "Lutris",          "Open gaming platform for Linux",               "games",        "#ff6c21"),
        mkbc("mangohud",        "MangoHud",        "Vulkan/OpenGL FPS & performance overlay",      "games",        "#e22020"),
    ]
}

fn mkbc(pkg: &str, name: &str, summary: &str, cat: &str, color: &str) -> AppRecord {
    AppRecord {
        id:             format!("bootc-{pkg}"),
        flatpak_id:     pkg.into(),
        name:           name.into(),
        summary:        summary.into(),
        description:    format!("{summary}\n\nSystem-level package installed via bootc (dnf5 install). A reboot is required after installation or removal."),
        version:        "latest".into(),
        developer:      "Fedora Project".into(),
        category:       cat.into(),
        icon_letter:    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "P".into()),
        icon_color_hex: color.into(),
        rating:         4.2,
        download_count: "N/A".into(),
        size_mb:        0.0,
        icon_url:       "".into(),
        backend:        AppBackend::Bootc,
    }
}
