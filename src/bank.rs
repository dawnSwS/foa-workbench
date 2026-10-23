use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use num_bigint::BigUint;
use quick_xml::events::BytesStart;

use crate::model::{BankReport, FIELDS};
use crate::scan::profile_handle;

#[path = "bank_write.rs"]
mod write;
pub use write::save_modified;

#[path = "bank_name.rs"]
mod name;
use name::read_xml;
pub use name::validate_player_name;

const ALPHABET: &[u8] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!$%/()=?,.-;:_^#+* @{[]}|~`";
const MAX_BANK_BYTES: u64 = 2 * 1024 * 1024;

pub fn parse_file(
    path: &Path,
    account_id: &str,
    server_id: Option<u32>,
) -> Result<BankReport, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BANK_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取文件：{error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BANK_BYTES {
        return Err("存档超过 2 MiB，已停止解析。".into());
    }
    let source_text =
        std::str::from_utf8(&bytes).map_err(|error| format!("存档不是有效的 UTF-8：{error}"))?;
    parse_text(source_text, path, account_id, server_id)
}

fn parse_text(
    source_text: &str,
    path: &Path,
    account_id: &str,
    server_id: Option<u32>,
) -> Result<BankReport, String> {
    let text = source_text.trim_start_matches('\u{feff}');
    let bank = read_xml(text)?;
    let version = bank.version.unwrap_or(0);
    check_version(version)?;
    let handle = profile_handle(path);
    let (fields, mut note) = if let Some(payload) = bank.payload {
        let fields = decode(&payload, version, handle.as_deref())?;
        let note = if bank.version.is_none() {
            "未声明 BankVersion，按地图脚本的版本 0 规则读取。".to_owned()
        } else {
            format!("已按版本 {version} 的字段顺序读取。")
        };
        (fields, note)
    } else {
        (
            BTreeMap::new(),
            "未找到 Encrypted / Bank 核心载荷；仅显示现有局内名字，可单独改名。".to_owned(),
        )
    };
    check_identity(path, &fields, &mut note);
    Ok(BankReport {
        player_name: bank.player_name,
        account_id: account_id.to_owned(),
        path: path.to_path_buf(),
        server_id,
        bank_version: version,
        fields,
        decode_note: note,
        signature_present: bank.signature,
        source_text: source_text.to_owned(),
    })
}

fn attribute(element: &BytesStart<'_>, key: &str) -> Result<Option<String>, String> {
    let mut value = None;
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|error| format!("XML 属性错误：{error}"))?;
        if attribute.key.as_ref().eq_ignore_ascii_case(key) {
            if value.is_some() {
                return Err("XML 含重复属性。".into());
            }
            value = Some(
                // Preserve stored whitespace instead of normalizing existing bank values.
                quick_xml::escape::unescape(&attribute.value)
                    .map_err(|error| format!("XML 转义错误：{error}"))?
                    .into_owned(),
            );
        }
    }
    Ok(value)
}

fn check_version(version: u32) -> Result<(), String> {
    if version > 6 {
        Err(format!("暂不支持 BankVersion {version}（支持 0–6）。"))
    } else {
        Ok(())
    }
}

fn ordinal(symbol: u8) -> Result<usize, String> {
    ALPHABET
        .iter()
        .position(|byte| *byte == symbol)
        .ok_or_else(|| format!("存档编码包含非法字符：0x{symbol:02X}"))
}

fn decrypt(code: &str, key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Err("旧版存档需要完整的玩家句柄。".into());
    }
    let mut result = String::with_capacity(code.len());
    for (symbol, key_byte) in code.bytes().zip(key.bytes().cycle()) {
        let index = (ordinal(symbol)? + ALPHABET.len() - ordinal(key_byte)?) % ALPHABET.len();
        let character = ALPHABET.get(index).ok_or("存档解密索引无效。")?;
        result.push(char::from(*character));
    }
    Ok(result)
}

pub fn decode(
    code: &str,
    version: u32,
    handle: Option<&str>,
) -> Result<BTreeMap<String, u64>, String> {
    check_version(version)?;
    if code.len() > 4096 {
        return Err("存档编码长度异常。".into());
    }
    let normalized = match version {
        0 | 1 => decrypt(
            code,
            &handle
                .ok_or("版本 0/1 需要位于 5-S2-1-… 这类玩家目录中的原始存档。")?
                .replace('-', ""),
        )?,
        2..=4 => decrypt(code, "A17Z")?,
        _ => code.to_owned(),
    };
    let mut number = BigUint::from(0_u8);
    for symbol in normalized.bytes() {
        let digit = u32::try_from(ordinal(symbol)?).map_err(|error| error.to_string())?;
        number = number * 89_u32 + BigUint::from(digit);
    }
    let mut fields = BTreeMap::new();
    for field in FIELDS.iter().rev().filter(|field| field.since <= version) {
        let base = BigUint::from(field.maximum(version) + 1);
        let value = (&number % &base)
            .to_u64_digits()
            .first()
            .copied()
            .unwrap_or(0);
        number /= base;
        fields.insert(field.key.to_owned(), value);
    }
    if number != BigUint::from(0_u8) {
        return Err("存档存在未消费的高位数据，版本、密钥或字段结构可能不匹配。".into());
    }
    Ok(fields)
}

fn check_identity(path: &Path, fields: &BTreeMap<String, u64>, note: &mut String) {
    let expected = profile_handle(path).and_then(|handle| {
        handle
            .rsplit('-')
            .next()
            .and_then(|id| id.parse::<u64>().ok())
    });
    if let (Some(expected), Some(actual)) = (expected, fields.get("player_id")) {
        if expected != *actual {
            note.push_str(" 存档玩家 ID 与所在目录的玩家句柄不一致。");
        }
    }
}

#[cfg(test)]
#[path = "bank_preset_tests.rs"]
mod preset_tests;
