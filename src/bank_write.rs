use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use num_bigint::BigUint;

use super::name::update_xml;
use super::{
    check_version, decode, ordinal, parse_text, validate_player_name, ALPHABET, MAX_BANK_BYTES,
};
use crate::edit_policy;
use crate::model::{BankReport, FIELDS};
use crate::scan::profile_handle;

pub struct SaveOutcome {
    pub report: BankReport,
    pub backup_path: Option<PathBuf>,
}

fn validate_fields(fields: &BTreeMap<String, u64>, version: u32) -> Result<(), String> {
    check_version(version)?;
    for (key, value) in fields {
        let field = FIELDS
            .iter()
            .find(|field| field.key == key && field.since <= version)
            .ok_or_else(|| format!("版本 {version} 不支持字段 {key}。"))?;
        if *value > field.maximum(version) {
            return Err(format!(
                "{} 必须在 0–{} 之间。",
                field.label,
                field.maximum(version)
            ));
        }
    }
    Ok(())
}

fn encrypt(code: &str, key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Err("旧版存档需要完整的玩家句柄。".into());
    }
    code.bytes()
        .zip(key.bytes().cycle())
        .map(|(symbol, key_byte)| {
            let index = (ordinal(symbol)? + ordinal(key_byte)?) % ALPHABET.len();
            ALPHABET
                .get(index)
                .copied()
                .map(char::from)
                .ok_or_else(|| "存档加密索引无效。".into())
        })
        .collect()
}

pub fn encode(
    fields: &BTreeMap<String, u64>,
    version: u32,
    handle: Option<&str>,
) -> Result<String, String> {
    validate_fields(fields, version)?;
    let mut number = BigUint::from(0_u8);
    for field in FIELDS.iter().filter(|field| field.since <= version) {
        let value = fields
            .get(field.key)
            .ok_or_else(|| format!("缺少字段 {}，不能生成完整游戏存档。", field.key))?;
        number = number * (field.maximum(version) + 1) + value;
    }
    let mut digits = Vec::new();
    // STARCODE_Base10ToN returns an empty string when the decimal value is zero.
    while number != BigUint::from(0_u8) {
        let digit = (&number % 89_u32)
            .to_u32_digits()
            .first()
            .copied()
            .unwrap_or(0);
        let index = usize::try_from(digit).map_err(|error| error.to_string())?;
        digits.push(*ALPHABET.get(index).ok_or("存档压缩索引无效。")?);
        number /= 89_u32;
    }
    digits.reverse();
    let code = String::from_utf8(digits).map_err(|error| error.to_string())?;
    match version {
        0 | 1 => encrypt(
            &code,
            &handle
                .ok_or("版本 0/1 需要原始玩家句柄。")?
                .replace('-', ""),
        ),
        2..=4 => encrypt(&code, "A17Z"),
        _ => Ok(code),
    }
}

fn verify_source(report: &BankReport) -> Result<(), String> {
    let current = fs::read(&report.path).map_err(|error| format!("无法重新读取源存档：{error}"))?;
    if current != report.source_text.as_bytes() {
        return Err("源存档已被游戏或其他程序修改，需重新打开后编辑。".into());
    }
    Ok(())
}

fn verified_baseline(report: &BankReport) -> Result<BankReport, String> {
    verify_source(report)?;
    // Public report fields are display state, not a trusted permission baseline.
    // Re-decode the exact disk-verified snapshot before checking any changes.
    let original = parse_text(
        &report.source_text,
        &report.path,
        &report.account_id,
        report.server_id,
    )?;
    if original.bank_version != report.bank_version
        || original.fields != report.fields
        || original.player_name != report.player_name
    {
        return Err("存档报告与原始文件不一致，需重新打开后编辑。".into());
    }
    Ok(original)
}

fn output_snapshot(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err("保存目标必须是普通文件，不能为目录或符号链接。".into())
        }
        Ok(_) => fs::read(path)
            .map(Some)
            .map_err(|error| format!("无法读取保存目标：{error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("无法检查保存目标：{error}")),
    }
}

static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn create_sibling(destination: &Path, suffix: &str) -> Result<(PathBuf, File), String> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .ok_or("保存目标缺少文件名。")?
        .to_string_lossy();
    for _ in 0..100 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!("{name}.{nanos}-{sequence}.{suffix}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("无法创建 {suffix} 文件：{error}")),
        }
    }
    Err("无法生成不重复的临时文件名。".into())
}

struct TemporaryFile(Option<PathBuf>);

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn write_synced(file: &mut File, bytes: &[u8]) -> Result<(), String> {
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("写入文件失败：{error}"))
}

fn validate_destination(
    report: &BankReport,
    destination: &Path,
    destination_exists: bool,
) -> Result<(), String> {
    if edit_policy::ROLE_BAN_ONLY && destination_exists {
        let source = report
            .path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let output = destination
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if source != output {
            return Err("普通版不允许覆盖其他已有文件，以保留其非封禁数据；仅支持修改当前文件或另存为新文件。".into());
        }
    }
    Ok(())
}

fn persist(
    report: &BankReport,
    destination: &Path,
    bytes: &[u8],
) -> Result<Option<PathBuf>, String> {
    let previous = output_snapshot(destination)?;
    validate_destination(report, destination, previous.is_some())?;
    let (temporary_path, mut temporary_file) = create_sibling(destination, "tmp")?;
    let mut temporary = TemporaryFile(Some(temporary_path.clone()));
    let staged = write_synced(&mut temporary_file, bytes);
    drop(temporary_file);
    staged?;
    verify_source(report)?;
    if output_snapshot(destination)? != previous {
        return Err("保存目标已被其他程序修改，需重新选择或打开。".into());
    }
    let backup_path = if let Some(previous) = &previous {
        let (path, mut backup) = create_sibling(destination, "bak")?;
        let mut cleanup = TemporaryFile(Some(path.clone()));
        let copied = write_synced(&mut backup, previous);
        drop(backup);
        copied?;
        cleanup.0 = None;
        Some(path)
    } else {
        None
    };
    // Verify again after backup creation, immediately before replacing the target.
    verify_source(report)?;
    if output_snapshot(destination)? != previous {
        return Err("保存目标在备份时发生变化，已停止保存。".into());
    }
    fs::rename(&temporary_path, destination)
        .map_err(|error| format!("替换存档失败，原文件未主动删除：{error}"))?;
    temporary.0 = None;
    Ok(backup_path)
}

pub fn save_modified(
    report: &BankReport,
    fields: &BTreeMap<String, u64>,
    player_name: Option<&str>,
    destination: &Path,
) -> Result<SaveOutcome, String> {
    // Display state is not the permission baseline: re-parse the disk snapshot.
    let original = verified_baseline(report)?;
    validate_fields(fields, original.bank_version)?;
    edit_policy::validate_changes(&original.fields, fields)?;
    if let Some(name) = player_name {
        validate_player_name(name)?;
    }
    if !destination
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("SC2Bank"))
    {
        return Err("保存文件的扩展名必须为 .SC2Bank。".into());
    }
    let handle = profile_handle(&report.path);
    if !fields.is_empty() && report.bank_version <= 1 && profile_handle(destination) != handle {
        return Err("版本 0/1 必须保存在同一玩家句柄目录下，否则无法正确解密。".into());
    }
    // A name-only edit must preserve even noncanonical encodings and legacy keys.
    let code = if original.fields == *fields {
        None
    } else {
        let code = encode(fields, original.bank_version, handle.as_deref())?;
        if decode(&code, original.bank_version, handle.as_deref())? != *fields {
            return Err("编码回读校验失败，未写入存档。".into());
        }
        Some(code)
    };
    let text = update_xml(&report.source_text, code.as_deref(), player_name)?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_BANK_BYTES {
        return Err("输出存档超过 2 MiB，未写入文件。".into());
    }
    let mut saved = parse_text(&text, destination, &report.account_id, report.server_id)?;
    let expected_name = player_name.or(original.player_name.as_deref());
    if saved.bank_version != original.bank_version
        || saved.fields != *fields
        || saved.player_name.as_deref() != expected_name
    {
        return Err("输出存档回读校验失败，未写入文件。".into());
    }
    let backup_path = persist(report, destination, text.as_bytes())?;
    // Verify the actual persisted bytes, not only the staged in-memory string.
    let disk = fs::read(destination).map_err(|error| format!("保存后无法回读存档：{error}"))?;
    if disk != text.as_bytes() {
        return Err(
            "保存后文件与已验证内容不一致；目标可能被其他程序修改。覆盖已有文件时生成的自动备份可用于恢复。".into(),
        );
    }
    saved.decode_note.push_str(" 已保存并通过存档回读校验。");
    if report.signature_present && !saved.signature_present {
        saved
            .decode_note
            .push_str(" 已移除因修改而失效的原 SC2 签名。");
    }
    Ok(SaveOutcome {
        report: saved,
        backup_path,
    })
}
