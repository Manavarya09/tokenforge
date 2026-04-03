use anyhow::Result;
use std::path::Path;

use crate::context::store::Store;

/// Build a project relevance profile from git history and file access patterns.
pub fn build_profile(project_dir: &Path, db_path: &Path) -> Result<ProfileReport> {
    let store = Store::open(db_path)?;
    let project_path = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf())
        .to_string_lossy()
        .to_string();

    // Get recently changed files from git
    let recent_files = get_recent_git_files(project_dir)?;

    // Record them in the profile
    for file in &recent_files {
        let _ = store.record_file_access(&project_path, file);
    }

    // Get top files from database
    let top_files = store.top_files(&project_path, 20)?;

    Ok(ProfileReport {
        project_path,
        recent_git_files: recent_files.len(),
        tracked_files: top_files.len(),
        top_files: top_files
            .into_iter()
            .map(|(path, count)| FileProfile {
                path,
                access_count: count,
            })
            .collect(),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct ProfileReport {
    pub project_path: String,
    pub recent_git_files: usize,
    pub tracked_files: usize,
    pub top_files: Vec<FileProfile>,
}

#[derive(Debug, serde::Serialize)]
pub struct FileProfile {
    pub path: String,
    pub access_count: i64,
}

fn get_recent_git_files(project_dir: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["log", "--name-only", "--pretty=format:", "-n", "50"])
        .current_dir(project_dir)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let files: Vec<String> = text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.to_string())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            Ok(files)
        }
        _ => Ok(Vec::new()), // Not a git repo or git not available
    }
}
