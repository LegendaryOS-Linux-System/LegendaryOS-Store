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

// ─── Flatpak catalog (primary) ────────────────────────────────────────────────

pub async fn fetch_flatpak_apps() -> Vec<AppRecord> {
    // Try local flatpak remote-ls first (fastest, most accurate)
    match fetch_via_flatpak_cli().await {
        Ok(apps) if apps.len() > 10 => {
            eprintln!("[catalog] flatpak remote-ls: {} apps", apps.len());
            return apps;
        }
        Ok(apps) => eprintln!("[catalog] flatpak remote-ls returned only {} apps, trying HTTP", apps.len()),
        Err(e)   => eprintln!("[catalog] flatpak remote-ls failed: {e}, trying HTTP"),
    }

    // HTTP fallback
    match fetch_via_http().await {
        Ok(apps) if !apps.is_empty() => {
            eprintln!("[catalog] Flathub HTTP: {} apps", apps.len());
            apps
        }
        Err(e) => {
            eprintln!("[catalog] Flathub HTTP error: {e} — using seed");
            flatpak_seed()
        }
        _ => {
            eprintln!("[catalog] Flathub HTTP empty — using seed");
            flatpak_seed()
        }
    }
}

/// Query local Flathub metadata via flatpak CLI.
/// This is what GNOME Software and Plasma Discover do under the hood.
/// Columns: application, name, description, version, branch, origin
async fn fetch_via_flatpak_cli()
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    // First ensure flathub remote exists
    let remotes = Command::new("flatpak")
        .args(["remotes", "--columns=name"])
        .output().await?;
    let remote_list = String::from_utf8_lossy(&remotes.stdout);
    if !remote_list.contains("flathub") {
        return Err("flathub remote not configured".into());
    }

    // Fetch app list — use user + system installations
    let out = Command::new("flatpak")
        .args([
            "remote-ls", "flathub",
            "--app",
            "--columns=application,name,description,version",
            "--system",
        ])
        .output().await?;

    if !out.status.success() {
        // Try user installation
        let out2 = Command::new("flatpak")
            .args([
                "remote-ls", "flathub",
                "--app",
                "--columns=application,name,description,version",
                "--user",
            ])
            .output().await?;
        if !out2.status.success() {
            return Err("flatpak remote-ls failed on both system and user".into());
        }
        return parse_flatpak_remote_ls(&out2.stdout);
    }

    parse_flatpak_remote_ls(&out.stdout)
}

fn parse_flatpak_remote_ls(raw: &[u8])
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    let text = String::from_utf8_lossy(raw);
    let mut apps = Vec::new();

    for line in text.lines() {
        // Tab-separated: app_id \t name \t description \t version
        let cols: Vec<&str> = line.splitn(4, '\t').collect();
        if cols.len() < 2 { continue; }

        let app_id = cols[0].trim();
        if app_id.is_empty() || app_id == "Application ID" { continue; }

        let name    = cols.get(1).map(|s| s.trim()).unwrap_or(app_id);
        let summary = cols.get(2).map(|s| s.trim()).unwrap_or("").to_string();
        let version = cols.get(3).map(|s| s.trim()).unwrap_or("").to_string();

        if name.is_empty() { continue; }

        let category = guess_category_from_id(app_id);
        let icon_letter = name.chars().next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".into());
        let icon_color = category_color(category).to_string();

        // Stable deterministic rating from app_id hash (until we get real ratings)
        let rating = pseudo_rating(app_id);

        apps.push(AppRecord {
            id:             app_id.replace('.', "-").to_lowercase(),
            flatpak_id:     app_id.to_string(),
            name:           name.to_string(),
            summary:        truncate(&summary, 120),
            description:    if summary.is_empty() {
                format!("{name} is available from Flathub as a sandboxed Flatpak application.")
            } else {
                summary.clone()
            },
            version:        if version.is_empty() { "latest".into() } else { version },
            developer:      extract_developer_from_id(app_id),
            category:       category.to_string(),
            icon_letter,
            icon_color_hex: icon_color,
            icon_url:       format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{app_id}.png"),
            rating,
            download_count: "N/A".into(),
            size_mb:        0.0,
            backend:        AppBackend::Flatpak,
        });
    }

    Ok(apps)
}

/// Guess category from reverse-DNS app ID.
fn guess_category_from_id(id: &str) -> &'static str {
    let id_lower = id.to_lowercase();

    // Known patterns
    if id_lower.contains("game") || id_lower.contains("chess") || id_lower.contains("sudoku")
    || id_lower.contains("mines") || id_lower.contains("solitaire") || id_lower.contains("teeworlds")
    || id_lower.contains("0ad") || id_lower.contains("openra") { return "games"; }

    if id_lower.contains("browser") || id_lower.contains("firefox") || id_lower.contains("chromium")
    || id_lower.contains("epiphany") || id_lower.contains("falkon") { return "internet"; }

    if id_lower.contains("chat") || id_lower.contains("signal") || id_lower.contains("telegram")
    || id_lower.contains("discord") || id_lower.contains("element") || id_lower.contains("irc")
    || id_lower.contains("slack") || id_lower.contains("zoom") || id_lower.contains("teams") { return "internet"; }

    if id_lower.contains("mail") || id_lower.contains("thunderbird") || id_lower.contains("geary")
    || id_lower.contains("evolution") { return "internet"; }

    if id_lower.contains("video") || id_lower.contains("vlc") || id_lower.contains("mpv")
    || id_lower.contains("totem") || id_lower.contains("celluloid") || id_lower.contains("clapper") { return "multimedia"; }

    if id_lower.contains("music") || id_lower.contains("audio") || id_lower.contains("rhythmbox")
    || id_lower.contains("lollypop") || id_lower.contains("spotif") || id_lower.contains("clementine")
    || id_lower.contains("audacity") || id_lower.contains("ardour") { return "multimedia"; }

    if id_lower.contains("obs") || id_lower.contains("kdenlive") || id_lower.contains("pitivi")
    || id_lower.contains("openshot") || id_lower.contains("handbrake") { return "multimedia"; }

    if id_lower.contains("gimp") || id_lower.contains("inkscape") || id_lower.contains("krita")
    || id_lower.contains("darktable") || id_lower.contains("rawtherapee") || id_lower.contains("blender")
    || id_lower.contains("pinta") || id_lower.contains("digikam") { return "graphics"; }

    if id_lower.contains("office") || id_lower.contains("libreoffice") || id_lower.contains("writer")
    || id_lower.contains("calc") || id_lower.contains("impress") || id_lower.contains("onlyoffice")
    || id_lower.contains("calligra") { return "productivity"; }

    if id_lower.contains("anki") || id_lower.contains("planner") || id_lower.contains("todo")
    || id_lower.contains("tasks") || id_lower.contains("notes") || id_lower.contains("obsidian") { return "productivity"; }

    if id_lower.contains("code") || id_lower.contains("studio") && id_lower.contains("android")
    || id_lower.contains("eclipse") || id_lower.contains("netbeans") || id_lower.contains("idea")
    || id_lower.contains("pycharm") || id_lower.contains("goland") || id_lower.contains("rider") { return "development"; }

    if id_lower.contains("gitg") || id_lower.contains("gitkraken") || id_lower.contains("sourcetree") { return "development"; }

    if id_lower.contains("steam") || id_lower.contains("lutris") || id_lower.contains("heroic")
    || id_lower.contains("bottles") { return "games"; }

    if id_lower.contains("terminal") || id_lower.contains("console") || id_lower.contains("tilix")
    || id_lower.contains("alacritty") || id_lower.contains("kitty") || id_lower.contains("wezterm") { return "tools"; }

    if id_lower.contains("boxes") || id_lower.contains("virt") || id_lower.contains("docker")
    || id_lower.contains("podman") || id_lower.contains("flatseal") || id_lower.contains("warehouse") { return "tools"; }

    // Fallback by TLD-style domain
    if id_lower.starts_with("org.gnome.") || id_lower.starts_with("org.kde.") {
        // Most GNOME/KDE core apps are tools
        return "tools";
    }

    "tools"
}

fn extract_developer_from_id(id: &str) -> String {
    // "com.visualstudio.code" → "visualstudio" → "Visual Studio"
    // "org.mozilla.firefox"   → "mozilla"       → "Mozilla"
    let parts: Vec<&str> = id.split('.').collect();
    if parts.len() >= 2 {
        let dev = parts[1];
        // Capitalize first letter
        let mut chars = dev.chars();
        match chars.next() {
            None    => dev.to_string(),
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        }
    } else {
        String::new()
    }
}

fn pseudo_rating(app_id: &str) -> f32 {
    // Known high-quality apps
    let stars5 = ["firefox","vlc","blender","signal","inkscape","krita","obs","libreoffice","audacity","gimp"];
    let name_lower = app_id.to_lowercase();
    for s in &stars5 {
        if name_lower.contains(s) { return 4.8; }
    }
    // Deterministic 3.5–4.7 from hash
    let hash: u32 = app_id.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32).wrapping_mul(31));
    3.5 + (hash % 13) as f32 * 0.1
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

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() }
    else              { s[..max].to_string() + "…" }
}

// ─── HTTP fallback (Flathub API v2) ──────────────────────────────────────────

async fn fetch_via_http()
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("gnome-software/46.0 flatpak/1.15.4 (X11; Linux x86_64)")
        .build()?;

    let mut all: Vec<AppRecord> = vec![];

    for page in 1u32..=2 {
        let url = format!(
            "https://flathub.org/api/v2/apps?page={page}&per_page=250&sort=installs"
        );
        let resp = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r)  => { eprintln!("[catalog] page {page} HTTP {}", r.status()); break; }
            Err(e) => { eprintln!("[catalog] page {page} error: {e}"); break; }
        };

        let raw: serde_json::Value = resp.json().await?;
        let hits = match raw["hits"].as_array() {
            Some(a) if !a.is_empty() => a,
            _ => break,
        };

        let records: Vec<AppRecord> = hits.iter().filter_map(parse_flathub_json).collect();
        all.extend(records);

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    Ok(all)
}

fn parse_flathub_json(v: &serde_json::Value) -> Option<AppRecord> {
    let app_id  = v["app_id"].as_str().or_else(|| v["id"].as_str())?;
    let name    = v["name"].as_str().unwrap_or(app_id);
    let summary = v["summary"].as_str().unwrap_or("").to_string();
    let desc    = v["description"].as_str().unwrap_or(&summary).to_string();
    let dev     = v["developer_name"].as_str()
                    .or_else(|| v["project_group"].as_str())
                    .unwrap_or("").to_string();
    let version = v["version"].as_str().unwrap_or("").to_string();
    let dl      = v["installs"].as_u64().map(fmt_count).unwrap_or_else(|| "N/A".into());
    let rating  = v["rating"].as_f64().map(|r| r as f32).unwrap_or_else(|| pseudo_rating(app_id));
    let cat     = v["categories"].as_array()
                    .and_then(|a| a.first()).and_then(|c| c.as_str())
                    .map(map_flathub_category).unwrap_or("tools").to_string();

    Some(AppRecord {
        id:             app_id.replace('.', "-").to_lowercase(),
        flatpak_id:     app_id.to_string(),
        name:           name.to_string(),
        summary:        truncate(&summary, 120),
        description:    truncate(&desc, 600),
        version:        if version.is_empty() { "latest".into() } else { version },
        developer:      dev,
        category:       cat.clone(),
        icon_letter:    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into()),
        icon_color_hex: category_color(&cat).to_string(),
        icon_url:       format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{app_id}.png"),
        rating,
        download_count: dl,
        size_mb:        0.0,
        backend:        AppBackend::Flatpak,
    })
}

fn map_flathub_category(raw: &str) -> &'static str {
    match raw.to_lowercase().as_str() {
        "webbrowser"|"network"|"chat"|"email"              => "internet",
        "video"|"audio"|"audiovideo"|"music"               => "multimedia",
        "graphics"|"photography"                           => "graphics",
        "office"|"productivity"|"finance"                  => "productivity",
        "development"|"ide"|"debugger"                     => "development",
        "game"|"games"|"emulator"                          => "games",
        "utility"|"utilities"|"settings"                   => "tools",
        "system"|"monitor"|"security"                      => "system",
        _                                                  => "tools",
    }
}

fn fmt_count(n: u64) -> String {
    if n >= 1_000_000 { format!("{:.1}M", n as f64 / 1_000_000.0) }
    else if n >= 1_000 { format!("{:.0}K", n as f64 / 1_000.0) }
    else { n.to_string() }
}

// ─── bootc / dnf5 ────────────────────────────────────────────────────────────

pub async fn fetch_bootc_packages() -> Vec<AppRecord> {
    match try_dnf5_repoquery().await {
        Ok(v) if !v.is_empty() => { eprintln!("[catalog] dnf5: {} packages", v.len()); v }
        Err(e) => { eprintln!("[catalog] dnf5 error: {e}"); bootc_seed() }
        _      => { eprintln!("[catalog] dnf5 empty"); bootc_seed() }
    }
}

async fn try_dnf5_repoquery()
    -> Result<Vec<AppRecord>, Box<dyn std::error::Error + Send + Sync>>
{
    let groups: &[(&str, &[&str])] = &[
        ("multimedia",   &["ffmpeg","gstreamer1-plugins-bad-free","gstreamer1-plugins-ugly",
                           "mkvtoolnix","lame","opus-tools","flac","x265","x264","libva-utils"]),
        ("development",  &["gcc","gcc-c++","clang","make","cmake","git","neovim","helix",
                           "golang","nodejs","python3-pip","podman","buildah","skopeo",
                           "rustup","java-21-openjdk","kotlin"]),
        ("tools",        &["fish","zsh","htop","btop","neofetch","tmux","screen","ranger",
                           "curl","wget","rsync","p7zip","unrar","fzf","bat","ripgrep","fd-find"]),
        ("system",       &["wireguard-tools","openssl","kernel-headers","dkms",
                           "fuse","fuse3","ntfs-3g","exfatprogs","powertop","thermald"]),
        ("internet",     &["neomutt","irssi","weechat","lynx","w3m","aria2","yt-dlp"]),
        ("games",        &["lutris","wine","winetricks","gamemode","mangohud","vkbasalt"]),
    ];

    let mut all: Vec<AppRecord> = vec![];

    for (category, pkgs) in groups {
        let out = Command::new("dnf5")
            .args(["repoquery", "--quiet",
                   "--qf", "%{name}\t%{summary}\t%{version}\t%{arch}\t%{installsize}"])
            .args(pkgs.iter())
            .output().await;

        let text = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            Ok(o) => {
                // dnf5 might not be available — try dnf
                let o2 = Command::new("dnf")
                    .args(["repoquery", "--quiet",
                           "--qf", "%{name}\t%{summary}\t%{version}\t%{arch}\t%{installsize}"])
                    .args(pkgs.iter())
                    .output().await;
                match o2 {
                    Ok(o2) if o2.status.success() => String::from_utf8_lossy(&o2.stdout).to_string(),
                    _ => { eprintln!("[catalog] dnf5/dnf failed for {category}: {:?}", String::from_utf8_lossy(&o.stderr).lines().next()); continue; }
                }
            }
            Err(e) => { eprintln!("[catalog] dnf5 spawn error: {e}"); continue; }
        };

        // Deduplicate by name (dnf can return multiple arches)
        let mut seen = std::collections::HashSet::new();
        for line in text.lines() {
            let p: Vec<&str> = line.splitn(5, '\t').collect();
            if p.len() < 2 { continue; }
            let name    = p[0].trim();
            let summary = p.get(1).map(|s| s.trim()).unwrap_or("");
            let version = p.get(2).map(|s| s.trim()).unwrap_or("");
            let size_b: f32 = p.get(4).and_then(|s| s.trim().parse().ok()).unwrap_or(0.0);

            if name.is_empty() || name.contains('%') { continue; }
            if !seen.insert(name.to_string()) { continue; }

            all.push(AppRecord {
                id:             format!("bootc-{name}"),
                flatpak_id:     name.to_string(),
                name:           name.to_string(),
                summary:        summary.to_string(),
                description:    format!("{}\n\nSystem-level package installed via bootc (dnf5). A reboot is required to activate.", summary),
                version:        version.to_string(),
                developer:      "Fedora Project".into(),
                category:       category.to_string(),
                icon_letter:    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "P".into()),
                icon_color_hex: "#16a34a".into(),
                icon_url:       "".into(),
                rating:         4.0 + (name.len() % 3) as f32 * 0.2,
                download_count: "N/A".into(),
                size_mb:        size_b / 1_048_576.0,
                backend:        AppBackend::Bootc,
            });
        }
    }

    Ok(all)
}

// ─── Seed lists ───────────────────────────────────────────────────────────────

pub fn flatpak_seed() -> Vec<AppRecord> {
    vec![
        mkfp("org.mozilla.firefox",               "Firefox",         "Fast, private & secure browser",          "internet",     "#e25c00","F",4.8,"2.1M"),
        mkfp("org.videolan.VLC",                  "VLC",             "The ultimate media player",               "multimedia",   "#e8861a","V",4.8,"1.8M"),
        mkfp("com.visualstudio.code",             "VS Code",         "Powerful code editor by Microsoft",       "development",  "#007acc","C",4.9,"3.2M"),
        mkfp("org.gimp.GIMP",                     "GIMP",            "Professional image editing",              "graphics",     "#5c8926","G",4.5,"980K"),
        mkfp("com.obsproject.Studio",             "OBS Studio",      "Free streaming & recording",              "multimedia",   "#6441a5","O",4.8,"1.2M"),
        mkfp("org.libreoffice.LibreOffice",       "LibreOffice",     "Complete office suite",                   "productivity", "#18a303","L",4.4,"1.5M"),
        mkfp("org.inkscape.Inkscape",             "Inkscape",        "Professional vector graphics",            "graphics",     "#000000","I",4.6,"620K"),
        mkfp("org.signal.Signal",                 "Signal",          "Private encrypted messenger",             "internet",     "#3a76f0","S",4.7,"890K"),
        mkfp("org.blender.Blender",               "Blender",         "3D creation suite",                      "graphics",     "#ea7600","B",4.9,"1.1M"),
        mkfp("com.valvesoftware.Steam",           "Steam",           "The ultimate gaming platform",            "games",        "#1b2838","S",4.6,"4.5M"),
        mkfp("org.kde.kdenlive",                  "Kdenlive",        "Non-linear video editor",                 "multimedia",   "#2196f3","K",4.3,"450K"),
        mkfp("org.mozilla.Thunderbird",           "Thunderbird",     "Email, calendar and chat",                "internet",     "#0a84ff","T",4.3,"760K"),
        mkfp("com.usebottles.bottles",            "Bottles",         "Run Windows software on Linux",           "tools",        "#9b59b6","B",4.5,"530K"),
        mkfp("com.github.tchx84.Flatseal",       "Flatseal",        "Manage Flatpak permissions",              "tools",        "#4caf50","F",4.7,"310K"),
        mkfp("org.kde.krita",                     "Krita",           "Digital painting & illustration",         "graphics",     "#3daee9","K",4.8,"870K"),
        mkfp("com.spotify.Client",                "Spotify",         "Music streaming",                         "multimedia",   "#1db954","S",4.5,"2.4M"),
        mkfp("org.gnome.Boxes",                   "Boxes",           "Virtual machine manager",                 "tools",        "#e01b24","B",4.3,"290K"),
        mkfp("net.ankiweb.Anki",                  "Anki",            "Spaced repetition flashcards",            "productivity", "#0095da","A",4.7,"410K"),
        mkfp("com.discordapp.Discord",            "Discord",         "Voice, video and text chat",              "internet",     "#5865f2","D",4.4,"3.1M"),
        mkfp("org.telegram.desktop",              "Telegram",        "Fast and secure messenger",               "internet",     "#2ca5e0","T",4.6,"2.8M"),
        mkfp("io.podman_desktop.PodmanDesktop",   "Podman Desktop",  "Container and Kubernetes manager",        "development",  "#892ca0","P",4.5,"150K"),
        mkfp("org.audacityteam.Audacity",         "Audacity",        "Free audio editor",                       "multimedia",   "#0080c0","A",4.3,"650K"),
        mkfp("org.darktable.Darktable",           "Darktable",       "Professional photo workflow",             "graphics",     "#3b3b3b","D",4.5,"340K"),
        mkfp("com.heroicgameslauncher.hgl",       "Heroic Games",    "GOG & Epic Games launcher",               "games",        "#c0392b","H",4.4,"320K"),
        mkfp("org.gnome.NetworkDisplays",         "Network Displays","Share screen to wireless displays",       "tools",        "#4a90d9","N",4.0,"120K"),
        mkfp("io.github.celluloid_player.Celluloid","Celluloid",     "Simple GTK mpv frontend",                 "multimedia",   "#e91e63","C",4.4,"200K"),
        mkfp("com.github.wwmm.easyeffects",      "EasyEffects",     "Audio effects for PipeWire",              "multimedia",   "#7c3aed","E",4.5,"280K"),
        mkfp("org.gnome.Shotwell",                "Shotwell",        "Personal photo manager",                  "graphics",     "#3465a4","S",4.1,"190K"),
        mkfp("com.belmoussaoui.Authenticator",   "Authenticator",   "Two-factor authentication codes",         "tools",        "#e74c3c","A",4.6,"150K"),
        mkfp("io.github.flattool.Warehouse",     "Warehouse",       "Manage all things Flatpak",               "tools",        "#7c3aed","W",4.6,"180K"),
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
        developer:      extract_developer_from_id(id),
        category:       cat.into(),
        icon_letter:    letter.into(),
        icon_color_hex: color.into(),
        icon_url:       format!("https://dl.flathub.org/repo/appstream/x86_64/icons/128x128/{id}.png"),
        rating,
        download_count: dl.into(),
        size_mb:        0.0,
        backend:        AppBackend::Flatpak,
    }
}

fn bootc_seed() -> Vec<AppRecord> {
    vec![
        mkbc("ffmpeg",          "FFmpeg",          "Complete multimedia codec framework",          "multimedia",   "#00a651"),
        mkbc("fish",            "Fish Shell",      "Friendly interactive shell",                   "tools",        "#4bb5c1"),
        mkbc("neovim",          "Neovim",          "Hyperextensible Vim-based text editor",        "development",  "#57a143"),
        mkbc("podman",          "Podman",          "Daemonless OCI container engine",              "development",  "#892ca0"),
        mkbc("wireguard-tools", "WireGuard",       "Fast, modern VPN kernel module + tools",       "system",       "#88171a"),
        mkbc("btop",            "btop",            "Resource monitor: CPU, RAM, disk, net",        "tools",        "#f97316"),
        mkbc("tmux",            "tmux",            "Terminal multiplexer: panes + sessions",       "tools",        "#22c55e"),
        mkbc("gcc",             "GCC",             "GNU Compiler Collection: C, C++, Fortran",     "development",  "#f59e0b"),
        mkbc("clang",           "Clang",           "LLVM C/C++ compiler with great diagnostics",   "development",  "#a78bfa"),
        mkbc("golang",          "Go",              "Fast statically-typed compiled language",       "development",  "#00acd7"),
        mkbc("nodejs",          "Node.js",         "JavaScript runtime built on V8",               "development",  "#5fa04e"),
        mkbc("lutris",          "Lutris",          "Open gaming platform for Linux",               "games",        "#ff6c21"),
        mkbc("mangohud",        "MangoHud",        "Vulkan/OpenGL FPS & GPU overlay",              "games",        "#e22020"),
        mkbc("ripgrep",         "ripgrep",         "Blazing-fast grep alternative (Rust)",         "tools",        "#d946ef"),
        mkbc("bat",             "bat",             "cat clone with syntax highlighting",           "tools",        "#e2b96c"),
        mkbc("fzf",             "fzf",             "Command-line fuzzy finder",                    "tools",        "#3b82f6"),
        mkbc("yt-dlp",          "yt-dlp",          "Download videos from YouTube and more",        "multimedia",   "#ff0000"),
        mkbc("aria2",           "aria2",           "Lightweight multi-protocol download utility",   "internet",     "#4a90d9"),
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
        icon_url:       "".into(),
        rating:         3.8 + (pkg.len() % 5) as f32 * 0.2,
        download_count: "N/A".into(),
        size_mb:        0.0,
        backend:        AppBackend::Bootc,
    }
}
