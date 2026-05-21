use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct BootcResult {
    pub success:        bool,
    pub output:         String,
    pub requires_reboot: bool,
}

// ─── Status ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct BootcStatus {
    /// Current booted image reference
    pub booted_image:    String,
    /// Current booted version / checksum
    pub booted_version:  String,
    /// Staged image (pending reboot), if any
    pub staged_image:    Option<String>,
    /// Packages layered via rpm-ostree
    pub layered_packages: Vec<String>,
    /// True when a staged deployment exists
    pub reboot_required: bool,
    /// Base image timestamp
    pub timestamp:       String,
}

/// Parse `rpm-ostree status --json` into BootcStatus.
pub async fn status() -> BootcStatus {
    let out = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output().await;

    let json_str = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return BootcStatus::default(),
    };

    parse_rpmostree_status(&json_str)
}

fn parse_rpmostree_status(json: &str) -> BootcStatus {
    let mut st = BootcStatus::default();

    // Parse deployments array
    // rpm-ostree status --json → { "deployments": [ {...}, {...} ] }
    // First entry = booted, second = staged (if exists)

    let find_field = |haystack: &str, key: &str| -> Option<String> {
        let needle = format!("\"{}\"", key);
        let start  = haystack.find(&needle)?;
        let after  = &haystack[start + needle.len()..];
        // Find : then value
        let colon  = after.find(':')?;
        let val    = after[colon + 1..].trim_start();
        if val.starts_with('"') {
            let end = val[1..].find('"')?;
            Some(val[1..end + 1].to_string())
        } else if val.starts_with('[') {
            // Array — grab until ]
            let end = val.find(']')?;
            Some(val[..=end].to_string())
        } else {
            // bool/number — grab until , or }
            let end = val.find(|c| c == ',' || c == '}').unwrap_or(val.len());
            Some(val[..end].trim().to_string())
        }
    };

    // Find first deployment block
    if let Some(dep_start) = json.find("\"deployments\"") {
        let arr = &json[dep_start..];
        if let Some(obj_start) = arr.find('{') {
            let block = &arr[obj_start..];

            // booted image
            if let Some(v) = find_field(block, "container-image-reference") {
                st.booted_image = v;
            } else if let Some(v) = find_field(block, "origin") {
                st.booted_image = v;
            }

            // version
            if let Some(v) = find_field(block, "version") { st.booted_version = v; }

            // timestamp
            if let Some(v) = find_field(block, "timestamp") { st.timestamp = v; }

            // layered packages
            if let Some(pkgs_raw) = find_field(block, "requested-packages") {
                st.layered_packages = parse_json_string_array(&pkgs_raw);
            }
            if st.layered_packages.is_empty() {
                if let Some(pkgs_raw) = find_field(block, "packages") {
                    st.layered_packages = parse_json_string_array(&pkgs_raw);
                }
            }

            // staged?
            if let Some(v) = find_field(block, "staged") {
                st.reboot_required = v == "true";
            }
        }

        // Check for second deployment (staged)
        let mut block_count = 0;
        for (i, c) in arr.char_indices() {
            if c == '{' { block_count += 1; }
            if block_count == 2 {
                let staged_block = &arr[i..];
                if let Some(v) = find_field(staged_block, "container-image-reference") {
                    st.staged_image = Some(v);
                    st.reboot_required = true;
                }
                break;
            }
        }
    }

    st
}

fn parse_json_string_array(raw: &str) -> Vec<String> {
    // Parse ["a","b","c"] style arrays
    raw.trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ─── List layered packages ────────────────────────────────────────────────────

/// Returns list of RPM package names currently layered via rpm-ostree.
pub async fn list_packages() -> Vec<String> {
    let st = status().await;
    if !st.layered_packages.is_empty() {
        return st.layered_packages;
    }

    // Fallback: rpm -qa on packages that are NOT in the base image
    // (rough heuristic: packages newer than base timestamp)
    rpm_layered_packages().await
}

async fn rpm_layered_packages() -> Vec<String> {
    // rpm-ostree db list shows all RPMs including base
    // We query rpm-ostree specifically for requested local packages
    let out = Command::new("rpm-ostree")
        .args(["status", "--pending-exit-77"])
        .output().await;
    // Just return anything in "requested-packages" from plain status
    let out2 = Command::new("rpm-ostree")
        .args(["status"])
        .output().await;
    if let Ok(o) = out2 {
        if o.status.success() {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut pkgs = vec![];
            let mut in_section = false;
            for line in text.lines() {
                if line.contains("LayeredPackages:") || line.contains("RequestedPackages:") {
                    in_section = true;
                    // Extract packages on same line
                    if let Some(rest) = line.splitn(2, ':').nth(1) {
                        for p in rest.split_whitespace() {
                            pkgs.push(p.trim().to_string());
                        }
                    }
                } else if in_section {
                    if line.starts_with(' ') || line.starts_with('\t') {
                        for p in line.split_whitespace() {
                            pkgs.push(p.trim().to_string());
                        }
                    } else {
                        in_section = false;
                    }
                }
            }
            if !pkgs.is_empty() { return pkgs; }
        }
    }
    // Final fallback: rpm -qa --qf "%{NAME}\n" filtered to likely-layered packages
    let _ = out;
    vec![]
}

// ─── Install (rpm-ostree layer) ───────────────────────────────────────────────

pub async fn install<F>(package_name: &str, mut progress_cb: F) -> BootcResult
where F: FnMut(f32) + Send + 'static,
{
    progress_cb(0.05);
    eprintln!("[bootc] rpm-ostree install {package_name}");

    // Try with pkexec first (shows native polkit dialog)
    // then fall back to plain rpm-ostree (user must have sudo)
    let spawn_result = Command::new("pkexec")
        .args(["rpm-ostree", "install", "--idempotent", "-y", package_name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawn_result {
        Ok(c) => c,
        Err(_) => {
            // pkexec not available or not needed (running as root)
            match Command::new("rpm-ostree")
                .args(["install", "--idempotent", "-y", package_name])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c)  => c,
                Err(e) => return BootcResult {
                    success: false,
                    output: format!("Failed to spawn rpm-ostree: {e}"),
                    requires_reboot: false,
                },
            }
        }
    };

    // Stream stderr for progress feedback
    let stderr = child.stderr.take().expect("no stderr");
    let mut reader = BufReader::new(stderr).lines();
    let mut n = 0u32;
    while let Ok(Some(line)) = reader.next_line().await {
        n += 1;
        // rpm-ostree prints stages: "Resolving dependencies" "Downloading" "Deploying"
        let p = if line.contains("Downloading") { 0.3 + (n as f32 / 30.0).min(0.4) }
                else if line.contains("Deploying") { 0.7 + (n as f32 / 20.0).min(0.2) }
                else { (0.05 + n as f32 / 50.0).min(0.85) };
        progress_cb(p);
        eprintln!("[bootc] {line}");
    }

    let success = child.wait().await.map(|s| s.success()).unwrap_or(false);
    progress_cb(if success { 1.0 } else { 0.0 });

    BootcResult {
        success,
        output: if success {
            format!("{package_name} layered — reboot required")
        } else {
            format!("rpm-ostree install {package_name} failed")
        },
        requires_reboot: success,
    }
}

// ─── Remove (rpm-ostree uninstall) ───────────────────────────────────────────

pub async fn remove(package_name: &str) -> BootcResult {
    eprintln!("[bootc] rpm-ostree uninstall {package_name}");

    let out = Command::new("pkexec")
        .args(["rpm-ostree", "uninstall", "-y", package_name])
        .output().await
        .or_else(|_| {
            std::process::Command::new("rpm-ostree")
                .args(["uninstall", "-y", package_name])
                .output()
        });

    match out {
        Ok(o) => BootcResult {
            success:         o.status.success(),
            output:          String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult {
            success: false, output: format!("Failed: {e}"), requires_reboot: false,
        },
    }
}

// ─── rpm-ostree upgrade (layers + base) ──────────────────────────────────────

pub async fn rpmostree_upgrade() -> BootcResult {
    eprintln!("[bootc] rpm-ostree upgrade");
    let out = Command::new("pkexec")
        .args(["rpm-ostree", "upgrade"])
        .output().await
        .or_else(|_| std::process::Command::new("rpm-ostree").args(["upgrade"]).output());
    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult { success: false, output: format!("rpm-ostree upgrade failed: {e}"), requires_reboot: false },
    }
}

// ─── bootc upgrade (pull new base image) ─────────────────────────────────────

pub async fn bootc_upgrade() -> BootcResult {
    eprintln!("[bootc] bootc upgrade");
    let out = Command::new("pkexec")
        .args(["bootc", "upgrade"])
        .output().await
        .or_else(|_| std::process::Command::new("bootc").args(["upgrade"]).output());
    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult { success: false, output: format!("bootc upgrade failed: {e}"), requires_reboot: false },
    }
}

// ─── bootc switch (change base image) ────────────────────────────────────────

pub async fn bootc_switch(image: &str) -> BootcResult {
    eprintln!("[bootc] bootc switch {image}");
    let out = Command::new("pkexec")
        .args(["bootc", "switch", image])
        .output().await
        .or_else(|_| std::process::Command::new("bootc").args(["switch", image]).output());
    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: true,
        },
        Err(e) => BootcResult { success: false, output: format!("bootc switch failed: {e}"), requires_reboot: false },
    }
}

// ─── Rollback ─────────────────────────────────────────────────────────────────

pub async fn rollback() -> BootcResult {
    eprintln!("[bootc] rpm-ostree rollback");
    let out = Command::new("pkexec")
        .args(["rpm-ostree", "rollback"])
        .output().await
        .or_else(|_| std::process::Command::new("rpm-ostree").args(["rollback"]).output());
    match out {
        Ok(o) => BootcResult {
            success: o.status.success(),
            output: String::from_utf8_lossy(&o.stdout).to_string(),
            requires_reboot: o.status.success(),
        },
        Err(e) => BootcResult { success: false, output: format!("rollback failed: {e}"), requires_reboot: false },
    }
}

// ─── Reboot check ────────────────────────────────────────────────────────────

pub async fn reboot_pending() -> bool {
    let st = status().await;
    st.reboot_required || st.staged_image.is_some()
}
