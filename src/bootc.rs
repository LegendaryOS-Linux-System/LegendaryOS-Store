use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct BootcResult {
    pub success: bool,
    pub output: String,
    pub requires_reboot: bool,
}

/// List packages currently in the active bootc deployment's additional layers.
/// We parse `bootc status` JSON output.
pub async fn list_packages() -> Vec<String> {
    // Try `bootc status --json` first
    let out = Command::new("bootc")
        .args(["status", "--json"])
        .output()
        .await;

    if let Ok(o) = out {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            return parse_bootc_status_packages(&text);
        }
    }

    // Fallback: check which known packages are installed via dnf5/rpm
    list_via_rpm().await
}

fn parse_bootc_status_packages(json: &str) -> Vec<String> {
    // bootc status --json: look for "requestedPackages" or "packages" arrays
    let mut pkgs = vec![];
    for key in &["requestedPackages", "packages", "layeredPackages"] {
        if let Some(start) = json.find(&format!("\"{key}\"")) {
            let slice = &json[start..];
            if let Some(arr_start) = slice.find('[') {
                if let Some(arr_end) = slice.find(']') {
                    let arr = &slice[arr_start + 1..arr_end];
                    for item in arr.split(',') {
                        let clean = item.trim().trim_matches('"').trim().to_string();
                        if !clean.is_empty() {
                            pkgs.push(clean);
                        }
                    }
                }
            }
        }
    }
    pkgs
}

async fn list_via_rpm() -> Vec<String> {
    let out = Command::new("rpm")
        .args(["-qa", "--qf", "%{NAME}\n"])
        .output()
        .await;

    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        }
        _ => vec![],
    }
}

/// Install a system package using pkexec + dnf5 (polkit auth dialog shown by desktop).
/// After install, a reboot is needed to fully integrate into the bootc image.
pub async fn install<F>(package_name: &str, mut progress_cb: F) -> BootcResult
where
    F: FnMut(f32) + Send + 'static,
{
    progress_cb(0.05);

    // Use pkexec so polkit shows a native auth dialog — no terminal needed
    let mut child = match Command::new("pkexec")
        .args([
            "dnf5", "install",
            "-y",
            "--best",
            "--allowerasing",
            package_name,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            // pkexec not available — try plain dnf5 (may fail without root)
            match Command::new("dnf5")
                .args(["install", "-y", package_name])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => return BootcResult {
                    success: false,
                    output: format!("Failed to spawn dnf5: {e}"),
                    requires_reboot: false,
                },
            }
        }
    };

    // Stream stderr for progress
    let stderr = child.stderr.take().expect("no stderr");
    let mut reader = BufReader::new(stderr).lines();
    let mut line_count = 0u32;

    while let Ok(Some(line)) = reader.next_line().await {
        line_count += 1;
        let progress = (0.05 + line_count as f32 / 60.0).min(0.92);
        progress_cb(progress);
        eprintln!("[bootc] {line}");
    }

    let status = child.wait().await;
    let success = status.map(|s| s.success()).unwrap_or(false);
    progress_cb(if success { 1.0 } else { 0.0 });

    BootcResult {
        success,
        output: if success {
            format!("{package_name} installed — reboot to finalise")
        } else {
            format!("Installation of {package_name} failed")
        },
        requires_reboot: success,
    }
}

/// Remove a system package via pkexec + dnf5.
pub async fn remove(package_name: &str) -> BootcResult {
    let out = Command::new("pkexec")
        .args(["dnf5", "remove", "-y", package_name])
        .output()
        .await
        .or_else(|_| {
            // sync fallback — fine since this is already async context
            std::process::Command::new("dnf5")
                .args(["remove", "-y", package_name])
                .output()
        });

    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult {
            success: false,
            output: format!("Failed: {e}"),
            requires_reboot: false,
        },
    }
}

/// Pull the latest bootc base image.
pub async fn upgrade() -> BootcResult {
    let out = Command::new("bootc")
        .args(["upgrade"])
        .output()
        .await;

    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult {
            success: false,
            output: format!("bootc upgrade failed: {e}"),
            requires_reboot: false,
        },
    }
}

/// Check if a reboot is needed (bootc has staged changes).
pub async fn reboot_pending() -> bool {
    let out = Command::new("bootc")
        .args(["status", "--json"])
        .output()
        .await;

    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            // bootc reports staged deployment when there's a pending update
            text.contains("\"staged\"") && !text.contains("\"staged\":null")
        }
        _ => false,
    }
}
