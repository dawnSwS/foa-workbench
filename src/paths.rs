use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Prefer the real Documents known folder (including redirection) for game saves.
pub fn default_root() -> Option<PathBuf> {
    let onedrive = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
        .into_iter()
        .filter_map(|variable| env::var_os(variable).filter(|value| !value.is_empty()))
        .map(PathBuf::from);
    let appdata = env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    find_default_root(dirs::document_dir(), dirs::home_dir(), onedrive, appdata)
}

fn find_default_root(
    documents: Option<PathBuf>,
    home: Option<PathBuf>,
    onedrive: impl IntoIterator<Item = PathBuf>,
    appdata: Option<PathBuf>,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(documents) = documents {
        candidates.push(documents.join("StarCraft II").join("Accounts"));
    }
    if let Some(home) = home {
        candidates.push(home.join("Documents/StarCraft II/Accounts"));
    }
    for base in onedrive {
        candidates.push(base.join("Documents/StarCraft II/Accounts"));
    }
    if let Some(base) = appdata {
        for publisher in ["Blizzard", "Blizzard Entertainment", ""] {
            candidates.push(base.join(publisher).join("StarCraft II").join("Accounts"));
        }
    }
    candidates.into_iter().find(|path| path.is_dir())
}

/// Start at the newest replay across all accounts, even when `current` is a
/// file or a subdirectory belonging to just one account.
pub fn replay_start_directory(current: Option<&Path>, default: Option<&Path>) -> Option<PathBuf> {
    let accounts = current
        .and_then(accounts_root)
        .or_else(|| default.and_then(accounts_root));
    if let Some(accounts) = accounts {
        return latest_replay_directory(&accounts).or(Some(accounts));
    }
    current.and_then(|path| {
        path.ancestors()
            .find(|ancestor| ancestor.is_dir())
            .map(Path::to_path_buf)
    })
}

fn accounts_root(path: &Path) -> Option<PathBuf> {
    if let Some(accounts) = path.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("Accounts"))
            && ancestor.is_dir()
    }) {
        return Some(accounts.to_path_buf());
    }
    [path.join("Accounts"), path.join("StarCraft II/Accounts")]
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

fn subdirectories(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect()
}

fn latest_replay_directory(accounts: &Path) -> Option<PathBuf> {
    // The game layout is Accounts/<account>/<profile>/Replays/<category>/... .
    // Visit replay trees only: Banks and other account data do not affect this.
    let mut pending = Vec::new();
    for account in subdirectories(accounts) {
        for profile in subdirectories(&account) {
            let replays = profile.join("Replays");
            if fs::symlink_metadata(&replays).is_ok_and(|metadata| metadata.is_dir()) {
                pending.push(replays);
            }
        }
    }
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("SC2Replay"))
            {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                // Either timestamp may be unsupported. Never substitute the
                // directory timestamp or current time for missing file data.
                let Some(time) = metadata.modified().ok().max(metadata.created().ok()) else {
                    continue;
                };
                let candidate = (time, path);
                // Full path breaks ties independently of directory enumeration.
                if latest.as_ref().is_none_or(|previous| candidate > *previous) {
                    latest = Some(candidate);
                }
            }
        }
    }
    latest.and_then(|(_, path)| path.parent().map(Path::to_path_buf))
}

/// Expand Windows-style environment variables without constructing a username path.
pub fn expand_input(input: &str) -> Result<PathBuf, String> {
    let input = input.trim().trim_matches('"');
    if input.is_empty() {
        return Err("扫描目录不能为空；路径支持 %APPDATA% 环境变量。".into());
    }
    let mut output = OsString::new();
    let mut remaining = input;
    while let Some(start) = remaining.find('%') {
        output.push(remaining.get(..start).unwrap_or_default());
        let tail = remaining.get(start + 1..).unwrap_or_default();
        let Some(end) = tail.find('%') else {
            return Err("环境变量格式不完整；格式示例：%APPDATA%。".into());
        };
        let key = tail.get(..end).unwrap_or_default();
        let value = env::var_os(key)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("环境变量 %{key}% 未设置，需手动指定目录。"))?;
        output.push(value);
        remaining = tail.get(end + 1..).unwrap_or_default();
    }
    output.push(remaining);
    Ok(PathBuf::from(output))
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
