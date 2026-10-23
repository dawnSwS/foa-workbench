//! Full-edition Bank preset for the audited 6c6561…51a9a map revision.
//! This repairs editable entry conditions; it cannot promise a room-wide win.
use std::collections::BTreeMap;

use crate::alien_selection::{hardcoded_game_ban, hardcoded_host_ban};
use crate::model::{BankReport, FIELDS};
use crate::{edit_policy, scan};

pub const LABEL: &str = "一键异形选人预设";
pub const LIMITS: &str = "此预设修正可编辑的开局条件，不保证每局当选。大厅报名的 100 分加成需在大厅设置；PvE、其他玩家分数、0–50 附加项及第二轮职位组合仍会影响结果。游戏下次保存会将 MMR 限制到 100–3000。";

#[derive(Clone)]
pub struct Change {
    pub key: String,
    pub label: &'static str,
    pub before: u64,
    pub after: u64,
    pub reason: &'static str,
}

#[derive(Clone)]
pub struct Preset {
    pub fields: BTreeMap<String, u64>,
    pub changes: Vec<Change>,
}

/// No mutation or partial repair on error. Do not infer ownership from payload ID.
pub fn availability(report: &BankReport) -> Result<String, String> {
    if edit_policy::ROLE_BAN_ONLY {
        return Err("异形选人预设仅供完整版使用；普通版不能修改 MMR 或统计字段。".into());
    }
    if report.source_text.is_empty() {
        return Err("仅本地 SC2Bank 可应用预设；录像快照为只读。".into());
    }
    if !(4..=6).contains(&report.bank_version) {
        return Err("此预设需要 Bank v4–v6；旧版缺少 MMR 或职位次数，无法完整修正资格。请先由游戏生成新版存档。".into());
    }
    if report.fields.is_empty() {
        return Err("存档没有可解码的统计载荷，不能应用异形选人预设。".into());
    }
    if !report
        .path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("RR.SC2Bank"))
    {
        return Err("请打开原始 RR.SC2Bank 主存档应用预设；地图优先读取 RR，修改 XX 或其他名称的文件可能不生效。".into());
    }
    let handle = scan::profile_handle(&report.path)
        .ok_or("无法确认玩家身份；请从包含实际玩家句柄（如 5-S2-1-…）的原始目录打开存档。")?;
    if hardcoded_game_ban(&handle) || hardcoded_host_ban(&handle) {
        return Err("此玩家句柄位于地图固定禁选名单；修改存档字段不能解除，未应用预设。".into());
    }
    Ok(handle)
}

pub fn apply(report: &BankReport, current: &BTreeMap<String, u64>) -> Result<Preset, String> {
    let handle = availability(report)?;
    let identity = handle
        .rsplit('-')
        .next()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|id| *id > 0 && *id <= 100_000_000)
        .ok_or("玩家句柄中的 ID 无效，未应用预设。")?;
    edit_policy::validate_changes(&report.fields, current)?;
    let expected: Vec<_> = FIELDS
        .iter()
        .filter(|field| field.since <= report.bank_version)
        .collect();
    if current.len() != expected.len() {
        return Err("存档字段不完整或含未知字段，不能应用预设。".into());
    }
    for field in expected {
        let value = current
            .get(field.key)
            .ok_or_else(|| format!("缺少{}，不能应用预设。", field.label))?;
        if *value > field.maximum(report.bank_version) {
            return Err(format!("{}超出当前存档版本范围，未应用预设。", field.label));
        }
    }
    let mut preset = Preset {
        fields: current.clone(),
        changes: Vec::new(),
    };
    preset.set(
        "player_id",
        identity,
        false,
        "匹配所在玩家目录，避免加载时因身份不符重置统计及 MMR",
    )?;
    preset.set(
        "lv_alienSelectionHMMR",
        9999,
        false,
        "按指定值设置，开局归一化后参与全房平均 MMR",
    )?;
    preset.set(
        "lv_alienSelectionAMMR",
        1650,
        false,
        "按指定值设置，满足胜场门槛后用于本人评分",
    )?;
    // Load adds one game; role assignment adds one occurrence before protection.
    preset.set(
        "lv_statsGamesPlayed",
        9,
        true,
        "多人开局计入本局后至少 10 场，解除新玩家保护",
    )?;
    preset.set(
        "lv_statsGamesWonHost",
        5,
        true,
        "异形主宰胜场至少 5；此版未保存最初的异形继任者胜场，确保采用 AMMR 分支",
    )?;
    for role in 1..=11 {
        preset.set(
            &format!("lv_statsGamesPlayedAsRole[{role}]"),
            3,
            true,
            "该职位分配计入本局后至少 4 次，解除新职位保护",
        )?;
    }
    for (key, reason) in [
        ("lv_modTKScoreReset", "防止开局将两种 MMR 重置为 100"),
        ("lv_modBan", "解除存档中的整局封禁条件"),
        ("lv_modAlienBan", "解除存档中的异形候选禁选条件"),
        ("lv_autoModTrollAlien", "清除会触发异形禁选的违规计数"),
        ("lv_autoModTemphostbangames", "清除临时异形主宰禁选场次"),
        ("lv_flagSpawnDepriortized", "消除异形降优先级的 25 分扣分"),
        ("lv_modLeftLastGame", "消除上局离开的 100 分扣分"),
    ] {
        preset.set(key, 0, false, reason)?;
    }
    edit_policy::validate_changes(&report.fields, &preset.fields)?;
    Ok(preset)
}

impl Preset {
    fn set(
        &mut self,
        key: &str,
        value: u64,
        at_least: bool,
        reason: &'static str,
    ) -> Result<(), String> {
        let field = FIELDS
            .iter()
            .find(|field| field.key == key)
            .ok_or("预设字段定义缺失。")?;
        let before = *self
            .fields
            .get(key)
            .ok_or_else(|| format!("缺少{}，不能应用预设。", field.label))?;
        let after = if at_least { before.max(value) } else { value };
        if before != after {
            self.fields.insert(key.to_owned(), after);
            self.changes.push(Change {
                key: key.to_owned(),
                label: field.label,
                before,
                after,
                reason,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "alien_preset_tests.rs"]
mod tests;
