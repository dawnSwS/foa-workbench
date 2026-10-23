use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bank;
use crate::model::BankReport;

pub struct BankFile {
    pub path: PathBuf,
    pub server_id: Option<u32>,
}

pub struct Account {
    pub id: String,
    pub path: PathBuf,
    pub servers: BTreeSet<u32>,
    pub banks: Vec<BankFile>,
}

#[derive(Default)]
pub struct ScanResult {
    pub accounts: Vec<Account>,
    pub reports: Vec<BankReport>,
    pub issues: Vec<String>,
}

pub fn run(root: &Path, target: &str) -> Result<ScanResult, String> {
    if target.trim().is_empty() {
        return Err("目标文件名不能为空。".into());
    }
    let mut result = ScanResult::default();
    if root.is_file() {
        let account_path = root
            .ancestors()
            .find(|path| {
                path.parent()
                    .is_some_and(|parent| name_is(parent, "Accounts"))
            })
            .or_else(|| root.parent())
            .ok_or("无法定位存档所在目录。")?;
        let mut account = new_account(account_path);
        account.banks.push(BankFile {
            path: root.to_path_buf(),
            server_id: infer_server(root),
        });
        result.accounts.push(account);
    } else if root.is_dir() {
        discover(root, target, &mut result)?;
    } else {
        return Err(format!("目录或文件不存在：{}", root.display()));
    }
    for account in &result.accounts {
        for file in &account.banks {
            match bank::parse_file(&file.path, &account.id, file.server_id) {
                Ok(report) => result.reports.push(report),
                Err(error) => result
                    .issues
                    .push(format!("{}\n{error}", file.path.display())),
            }
        }
    }
    Ok(result)
}

fn discover(root: &Path, target: &str, result: &mut ScanResult) -> Result<(), String> {
    let mut roots = Vec::new();
    if name_is(root, "Accounts") {
        roots.push(root.to_path_buf());
    } else if root.join("Banks").is_dir() || is_account_name(root) {
        let mut account = new_account(root);
        walk(root, target, 0, &mut account, &mut result.issues);
        result.accounts.push(account);
        return Ok(());
    } else {
        find_accounts(root, 0, &mut roots, &mut result.issues);
    }
    if roots.is_empty() {
        return Err(format!(
            "在 {} 下未找到 Accounts 目录，需指定游戏存档目录。",
            root.display()
        ));
    }
    roots.sort();
    roots.dedup();
    for root in roots {
        for path in children(&root, &mut result.issues) {
            if is_account_name(&path) && path.is_dir() {
                let mut account = new_account(&path);
                walk(&path, target, 0, &mut account, &mut result.issues);
                account
                    .banks
                    .sort_by(|left, right| left.path.cmp(&right.path));
                result.accounts.push(account);
            }
        }
    }
    result
        .accounts
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

fn new_account(path: &Path) -> Account {
    Account {
        id: path.file_name().map_or_else(
            || "手动打开".into(),
            |name| name.to_string_lossy().into_owned(),
        ),
        path: path.to_path_buf(),
        servers: BTreeSet::new(),
        banks: Vec::new(),
    }
}

fn children(root: &Path, issues: &mut Vec<String>) -> Vec<PathBuf> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(format!("无法读取 {}：{error}", root.display()));
            return Vec::new();
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => match entry.file_type() {
                Ok(kind) if !kind.is_symlink() => paths.push(entry.path()),
                Ok(_) => {}
                Err(error) => issues.push(format!("无法读取文件属性：{error}")),
            },
            Err(error) => issues.push(format!("无法读取目录项：{error}")),
        }
    }
    paths.sort();
    paths
}

fn find_accounts(root: &Path, depth: u8, found: &mut Vec<PathBuf>, issues: &mut Vec<String>) {
    if depth > 5 {
        return;
    }
    for path in children(root, issues)
        .into_iter()
        .filter(|path| path.is_dir())
    {
        if name_is(&path, "Accounts") {
            found.push(path);
        } else {
            find_accounts(&path, depth + 1, found, issues);
        }
    }
}

fn walk(root: &Path, target: &str, depth: u8, account: &mut Account, issues: &mut Vec<String>) {
    if depth > 14 {
        issues.push(format!("目录嵌套超过 14 层，已跳过：{}", root.display()));
        return;
    }
    for path in children(root, issues) {
        if path.is_dir() {
            if let Some(id) = infer_server(&path) {
                account.servers.insert(id);
            }
            walk(&path, target, depth + 1, account, issues);
        } else if target_matches(&path, target) {
            let server_id = infer_server(&path);
            if let Some(id) = server_id {
                account.servers.insert(id);
            }
            account.banks.push(BankFile { path, server_id });
        }
    }
}

fn target_matches(path: &Path, target: &str) -> bool {
    let target = target.trim().replace('\\', "/").to_ascii_lowercase();
    if target.contains('/') {
        let path = path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase();
        path.ends_with(&format!("/{target}"))
    } else {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&target))
    }
}

fn name_is(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|part| part.to_string_lossy().eq_ignore_ascii_case(name))
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_account_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| digits(&name.to_string_lossy()))
}

fn profile_server(name: &str) -> Option<u32> {
    let mut parts = name.split('-');
    let (server, marker, shard, player) =
        (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_none()
        && marker.eq_ignore_ascii_case("S2")
        && digits(shard)
        && digits(player)
    {
        server.parse().ok()
    } else {
        None
    }
}

pub fn profile_handle(path: &Path) -> Option<String> {
    path.components().find_map(|component| {
        let name = component.as_os_str().to_string_lossy();
        profile_server(&name).map(|_| name.into_owned())
    })
}

fn infer_server(path: &Path) -> Option<u32> {
    if let Some(handle) = profile_handle(path) {
        return profile_server(&handle);
    }
    for ancestor in path.ancestors() {
        let name = ancestor.file_name()?.to_string_lossy();
        if ancestor
            .parent()
            .is_some_and(|parent| name_is(parent, "Actions"))
            && digits(&name)
        {
            return name.parse().ok();
        }
    }
    None
}

pub fn server_name(id: Option<u32>) -> String {
    match id {
        Some(1) => "美洲 / US".into(),
        Some(2) => "欧洲 / EU".into(),
        Some(3) => "韩国 / KR".into(),
        Some(4) => "台湾 / TW".into(),
        Some(5) => "中国大陆 / CN".into(),
        Some(6) => "东南亚 / SEA".into(),
        Some(id) => format!("未知区服 / {id}"),
        None => "区服未知".into(),
    }
}
