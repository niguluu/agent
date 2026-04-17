// overlayfs per-agent sandbox
//
// implements the recommended direction from docs/beyond-git-worktrees:
// each agent gets a thin overlayfs mount over the real repo
//   lower = repo root (read only)
//   upper = per task scratch
//   work  = overlayfs work dir (required by the kernel)
//   merged = where the agent actually runs
//
// on finish, the upper dir alone holds everything the agent wrote,
// so we can diff it or just drop it.
//
// linux only. needs CAP_SYS_ADMIN, so in practice `sudo mount`.
// callers should fall back to a plain git worktree if `mount_overlay`
// returns Err.

use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct OverlayPaths {
    pub lower: String,
    pub upper: String,
    pub work: String,
    pub merged: String,
    pub root: String, // parent dir that holds upper/work (easy to nuke)
}

impl OverlayPaths {
    pub fn for_task(repo_root: &str, id: usize, merged: &str) -> Self {
        let root = format!("../agent-overlay-{}", id);
        Self {
            lower: repo_root.to_string(),
            upper: format!("{}/upper", root),
            work: format!("{}/work", root),
            merged: merged.to_string(),
            root,
        }
    }
}

pub fn is_enabled() -> bool {
    // opt in. overlayfs needs root on most setups, so keep it off by default.
    std::env::var("JUNIE_OVERLAY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub async fn mount_overlay(paths: &OverlayPaths) -> Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("overlayfs is linux only".to_string());
    }

    for dir in [&paths.upper, &paths.work, &paths.merged] {
        fs::create_dir_all(dir)
            .await
            .map_err(|err| format!("mkdir {} failed {}", dir, err))?;
    }

    let lower_abs = absolutize(&paths.lower);
    let upper_abs = absolutize(&paths.upper);
    let work_abs = absolutize(&paths.work);
    let merged_abs = absolutize(&paths.merged);

    let options = format!(
        "lowerdir={},upperdir={},workdir={}",
        lower_abs.display(),
        upper_abs.display(),
        work_abs.display(),
    );

    let out = Command::new("sudo")
        .args([
            "-n",
            "mount",
            "-t",
            "overlay",
            "overlay",
            "-o",
            &options,
            &merged_abs.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|err| format!("spawn mount failed {}", err))?;

    if !out.status.success() {
        return Err(format!(
            "overlay mount failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    Ok(())
}

pub async fn unmount_overlay(paths: &OverlayPaths) -> Result<(), String> {
    let merged_abs = absolutize(&paths.merged);
    let out = Command::new("sudo")
        .args(["-n", "umount", &merged_abs.to_string_lossy()])
        .output()
        .await
        .map_err(|err| format!("spawn umount failed {}", err))?;

    if !out.status.success() {
        return Err(format!(
            "overlay umount failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    Ok(())
}

// drop the per-task upper/work dirs. safe to call after unmount.
pub async fn drop_scratch(paths: &OverlayPaths) {
    let _ = fs::remove_dir_all(&paths.root).await;
}

fn absolutize(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}
