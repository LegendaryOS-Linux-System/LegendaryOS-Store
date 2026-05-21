use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug)]
pub struct FlatpakResult {
    pub success: bool,
    pub output:  String,
}

// ─── List installed ───────────────────────────────────────────────────────────

pub async fn list_installed() -> Vec<String> {
    let out = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output().await;
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != "Application ID")
                .collect()
        }
        _ => vec![],
    }
}

// ─── List updates available ───────────────────────────────────────────────────

pub async fn list_updates() -> Vec<String> {
    let out = Command::new("flatpak")
        .args(["remote-ls", "--updates", "--app", "--columns=application", "flathub"])
        .output().await;
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != "Application ID")
                .collect()
        }
        _ => vec![],
    }
}

// ─── Installed sizes ──────────────────────────────────────────────────────────
// Returns HashMap<flatpak_id, size_in_MB>

pub async fn installed_sizes(ids: &[String]) -> std::collections::HashMap<String, f32> {
    let mut map = std::collections::HashMap::new();
    if ids.is_empty() { return map; }

    // flatpak info --show-size returns bytes
    for id in ids {
        let out = Command::new("flatpak")
            .args(["info", "--show-size", id])
            .output().await;
        if let Ok(o) = out {
            if o.status.success() {
                let text = String::from_utf8_lossy(&o.stdout);
                // Output like: "123456789" (bytes)
                if let Ok(bytes) = text.trim().parse::<f64>() {
                    map.insert(id.clone(), (bytes / 1_048_576.0) as f32);
                }
            }
        }
    }
    map
}

// ─── Install ─────────────────────────────────────────────────────────────────

pub async fn install<F>(flatpak_id: &str, mut progress_cb: F) -> FlatpakResult
where F: FnMut(f32) + Send + 'static,
{
    let mut child = match Command::new("flatpak")
        .args(["install", "--assumeyes", "--noninteractive", "flathub", flatpak_id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c)  => c,
        Err(e) => return FlatpakResult { success: false, output: format!("spawn failed: {e}") },
    };

    let stderr = child.stderr.take().expect("no stderr");
    let mut reader = BufReader::new(stderr).lines();
    let mut n = 0u32;
    while let Ok(Some(line)) = reader.next_line().await {
        n += 1;
        progress_cb((n as f32 / 60.0).min(0.92));
        eprintln!("[flatpak] {line}");
    }

    let success = child.wait().await.map(|s| s.success()).unwrap_or(false);
    progress_cb(if success { 1.0 } else { 0.0 });
    FlatpakResult {
        success,
        output: if success { format!("{flatpak_id} installed") } else { format!("install of {flatpak_id} failed") },
    }
}

// ─── Remove ──────────────────────────────────────────────────────────────────

pub async fn remove(flatpak_id: &str) -> FlatpakResult {
    let out = Command::new("flatpak")
        .args(["remove", "--assumeyes", "--noninteractive", flatpak_id])
        .output().await;
    match out {
        Ok(o) => FlatpakResult { success: o.status.success(), output: String::from_utf8_lossy(&o.stdout).to_string() },
        Err(e) => FlatpakResult { success: false, output: format!("spawn failed: {e}") },
    }
}

// ─── Update single ────────────────────────────────────────────────────────────

pub async fn update(flatpak_id: &str) -> FlatpakResult {
    let out = Command::new("flatpak")
        .args(["update", "--assumeyes", "--noninteractive", flatpak_id])
        .output().await;
    match out {
        Ok(o) => FlatpakResult { success: o.status.success(), output: String::from_utf8_lossy(&o.stdout).to_string() },
        Err(e) => FlatpakResult { success: false, output: format!("spawn failed: {e}") },
    }
}

// ─── Disk usage ──────────────────────────────────────────────────────────────
// Returns (total_mb, reclaimable_mb, Vec<(label, mb, color_hex)>)

pub async fn disk_usage() -> (f32, f32, Vec<(String, f32, String)>) {
    // flatpak --version to check it works
    let _du = Command::new("flatpak")
        .args(["--ostree-verbose", "du", "--all"])
        .output().await;

    // Primary: use du on flatpak dirs
    let flatpak_dirs = [
        ("/var/lib/flatpak",             "System apps",  "#7c3aed"),
        (&format!("{}/.local/share/flatpak", std::env::var("HOME").unwrap_or_default()), "User apps", "#d946ef"),
        ("/var/lib/flatpak/runtime",     "Runtimes",     "#3b82f6"),
        ("/var/lib/flatpak/repo",        "OSTree repo",  "#6b7280"),
    ];

    let mut breakdown: Vec<(String, f32, String)> = vec![];
    let mut total = 0.0f32;

    for (path, label, color) in &flatpak_dirs {
        let out = Command::new("du")
            .args(["-sb", path])
            .output().await;
        if let Ok(o) = out {
            if o.status.success() {
                let text = String::from_utf8_lossy(&o.stdout);
                if let Some(bytes_str) = text.split_whitespace().next() {
                    if let Ok(bytes) = bytes_str.parse::<f64>() {
                        let mb = (bytes / 1_048_576.0) as f32;
                        if mb > 0.1 {
                            breakdown.push((label.to_string(), mb, color.to_string()));
                            total += mb;
                        }
                    }
                }
            }
        }
    }

    // Reclaimable: unused runtimes
    let reclaimable = estimate_reclaimable().await;

    if breakdown.is_empty() {
        // Fallback: query flatpak info
        breakdown.push(("Flatpak data".into(), 0.0, "#7c3aed".into()));
    }

    (total, reclaimable, breakdown)
}

async fn estimate_reclaimable() -> f32 {
    // Count unused runtimes (runtimes with no apps depending on them)
    let out = Command::new("flatpak")
        .args(["list", "--runtime", "--columns=application,version"])
        .output().await;
    match out {
        Ok(o) if o.status.success() => {
            let count = String::from_utf8_lossy(&o.stdout).lines().count() as f32;
            // Very rough: each unused runtime ~200MB
            (count * 0.1 * 200.0).min(2000.0)
        }
        _ => 0.0,
    }
}

// ─── Cleanup ─────────────────────────────────────────────────────────────────

pub async fn cleanup() {
    // Remove unused runtimes and refs
    let _ = Command::new("flatpak")
        .args(["uninstall", "--assumeyes", "--noninteractive", "--unused"])
        .output().await;
    // Prune OSTree repo
    let _ = Command::new("flatpak")
        .args(["repair", "--user"])
        .output().await;
    eprintln!("[flatpak] cleanup done");
}
