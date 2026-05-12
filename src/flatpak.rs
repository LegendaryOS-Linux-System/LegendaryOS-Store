use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Result of a flatpak operation
#[derive(Debug)]
pub struct FlatpakResult {
    pub success: bool,
    pub output: String,
}

/// List all installed flatpak application IDs (user + system).
pub async fn list_installed() -> Vec<String> {
    let out = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
        .await;

    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != "Application ID")
                .collect()
        }
        _ => vec![],
    }
}

/// Check whether a single flatpak ID is currently installed.
pub async fn is_installed(flatpak_id: &str) -> bool {
    let ids = list_installed().await;
    ids.iter().any(|id| id == flatpak_id)
}

/// Install a Flatpak from Flathub, streaming progress lines via callback.
pub async fn install<F>(flatpak_id: &str, mut progress_cb: F) -> FlatpakResult
where
    F: FnMut(f32) + Send + 'static,
{
    let mut child = match Command::new("flatpak")
        .args([
            "install",
            "--assumeyes",
            "--noninteractive",
            "flathub",
            flatpak_id,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return FlatpakResult {
                success: false,
                output: format!("Failed to spawn flatpak: {e}"),
            }
        }
    };

    // Read stderr (flatpak prints progress there)
    let stderr = child.stderr.take().expect("no stderr");
    let mut reader = BufReader::new(stderr).lines();

    // Simulate progress based on line count (flatpak doesn't emit percentages)
    let mut line_count = 0u32;
    while let Ok(Some(line)) = reader.next_line().await {
        line_count += 1;
        // Rough heuristic: first 40 lines = download, rest = deploy
        let progress = (line_count as f32 / 60.0).min(0.95);
        progress_cb(progress);
        eprintln!("[flatpak] {line}");
    }

    let status = child.wait().await;
    let success = status.map(|s| s.success()).unwrap_or(false);
    progress_cb(if success { 1.0 } else { 0.0 });

    FlatpakResult {
        success,
        output: if success {
            format!("{flatpak_id} installed successfully")
        } else {
            format!("Installation of {flatpak_id} failed")
        },
    }
}

/// Remove a Flatpak application.
pub async fn remove(flatpak_id: &str) -> FlatpakResult {
    let out = Command::new("flatpak")
        .args([
            "remove",
            "--assumeyes",
            "--noninteractive",
            flatpak_id,
        ])
        .output()
        .await;

    match out {
        Ok(o) => FlatpakResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
        },
        Err(e) => FlatpakResult {
            success: false,
            output: format!("Failed to spawn flatpak: {e}"),
        },
    }
}

/// Update a single Flatpak application.
pub async fn update(flatpak_id: &str) -> FlatpakResult {
    let out = Command::new("flatpak")
        .args([
            "update",
            "--assumeyes",
            "--noninteractive",
            flatpak_id,
        ])
        .output()
        .await;

    match out {
        Ok(o) => FlatpakResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
        },
        Err(e) => FlatpakResult {
            success: false,
            output: format!("Failed to spawn flatpak: {e}"),
        },
    }
}

/// List Flatpak IDs that have updates available.
pub async fn list_updates() -> Vec<String> {
    let out = Command::new("flatpak")
        .args(["remote-ls", "--updates", "--app", "--columns=application", "flathub"])
        .output()
        .await;

    match out {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && l != "Application ID")
                .collect()
        }
        _ => vec![],
    }
}

/// Get metadata for a single app from Flathub remote info.
pub async fn app_info(flatpak_id: &str) -> Option<AppInfo> {
    let out = Command::new("flatpak")
        .args(["remote-info", "flathub", flatpak_id])
        .output()
        .await
        .ok()?;

    if !out.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut version = String::from("unknown");
    let mut size_mb = 0.0f32;

    for line in text.lines() {
        if let Some(v) = line.strip_prefix("        Version:") {
            version = v.trim().to_string();
        }
        if let Some(s) = line.strip_prefix("      Download:") {
            // Parse something like "45.2 MB"
            let s = s.trim();
            if let Some(num) = s.split_whitespace().next() {
                size_mb = num.parse().unwrap_or(0.0);
            }
        }
    }

    Some(AppInfo { version, size_mb })
}

#[derive(Debug, Clone)]
pub struct AppInfo {
    pub version: String,
    pub size_mb: f32,
}
