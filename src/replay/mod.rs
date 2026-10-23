//! Read-only replay bank snapshots. A snapshot is evidence of a synchronized
//! bank, not a promise that the engine accepted it or saved it after the game.
mod evidence;
mod mpq;
mod opening;
mod protocol;
mod selection;

pub use selection::AlienSelectionReview;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::bank;
use crate::model::BankReport;

const MAX_REPLAY_BYTES: u64 = 64 * 1024 * 1024;
const PROTOCOL_BUILD: u32 = 98_310;

pub struct ReplayReport {
    pub alien_selection: AlienSelectionReview,
    pub path: PathBuf,
    pub title: String,
    pub base_build: u32,
    pub protocol_build: u32,
    pub elapsed_game_loops: u32,
    pub bank_event_count: usize,
    pub players: Vec<ReplayPlayer>,
    pub warnings: Vec<String>,
    pub unmapped_banks: Vec<ReplayBank>,
    /// Unmodified protocol values, including raw byte arrays and event order.
    pub bank_events: Vec<Value>,
    /// Kept separately from Bank snapshots: this proves loading has finished,
    /// not that an absent historical payload contains recorded zeroes.
    loading_sync_events: Vec<Value>,
}

pub struct ReplayPlayer {
    pub slot_id: Option<u32>,
    pub user_id: Option<u32>,
    pub name: String,
    pub toon_handle: String,
    pub banks: Vec<ReplayBank>,
    pub selected_bank: Option<usize>,
    pub warnings: Vec<String>,
}

pub struct ReplayBank {
    /// Repeated core keys cannot establish a unique selection-time input.
    pub ambiguous_core: bool,
    pub name: String,
    pub gameloop: u32,
    pub end_gameloop: Option<u32>,
    pub toon_handle: String,
    pub signature: Vec<u8>,
    pub sections: BTreeMap<String, BTreeMap<String, ReplayKey>>,
    pub report: Option<BankReport>,
    pub warnings: Vec<String>,
}

#[derive(PartialEq, Eq)]
pub struct ReplayKey {
    pub value_type: u32,
    pub data: String,
    pub data_is_utf8: bool,
    pub values: Vec<ReplayValue>,
}

#[derive(PartialEq, Eq)]
pub struct ReplayValue {
    pub name: String,
    pub value_type: u32,
    pub data: String,
    pub data_is_utf8: bool,
}

impl ReplayBank {
    pub fn player_name(&self) -> Option<&str> {
        let key = self.sections.get("FA")?.get("Name")?;
        match key.value_type {
            3 if key.data_is_utf8 && key.values.is_empty() => Some(&key.data),
            7 if key.values.len() == 1 => key
                .values
                .first()
                .filter(|value| {
                    value.name == "Value" && value.value_type == 3 && value.data_is_utf8
                })
                .map(|value| value.data.as_str()),
            _ => None,
        }
    }
}

pub fn parse_file(path: &Path) -> Result<ReplayReport, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_REPLAY_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取录像：{error}"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REPLAY_BYTES {
        return Err("录像超过 64 MiB，已停止解析。".into());
    }
    parse_bytes(path, &bytes)
}

fn parse_bytes(path: &Path, bytes: &[u8]) -> Result<ReplayReport, String> {
    let header = protocol::header(mpq::user_data(bytes)?)?;
    let base_build = number(
        header.get("m_version").ok_or("录像缺少版本。")?,
        "m_baseBuild",
    )?;
    // 98370 has no published Blizzard schema. It is an explicit compatibility
    // profile verified against the supplied replay, never a nearest-build guess.
    if !matches!(base_build, PROTOCOL_BUILD | 98_370) {
        return Err(format!(
            "暂不支持录像基础版本 {base_build}；支持 98310，及已验证样本的 98370 兼容模式。"
        ));
    }
    let details = protocol::details(&mpq::extract(bytes, "replay.details")?)?;
    let init = protocol::init_data(&mpq::extract(bytes, "replay.initData")?)?;
    let events = protocol::game_events(&mpq::extract(bytes, "replay.game.events")?)?;
    let mut result = ReplayReport {
        alien_selection: AlienSelectionReview::default(),
        path: path.to_path_buf(),
        title: text(&details, "m_title")?,
        base_build,
        protocol_build: PROTOCOL_BUILD,
        elapsed_game_loops: number(&header, "m_elapsedGameLoops")?,
        bank_event_count: events.banks.len(),
        players: players(&details, &init)?,
        warnings: vec!["数据为录像中实际同步的存档快照；不代表本局结算存档，也未验证引擎签名或重演地图对统计的修改。未记录的字段不会补零。".into()],
        unmapped_banks: Vec::new(),
        bank_events: events.banks,
        loading_sync_events: events.loading_sync,
    };
    if base_build != PROTOCOL_BUILD {
        result.warnings.push("此录像基础版本为 98370，使用 Blizzard 98310 协议兼容解码；该组合已通过所附样本的完整事件流校验，其他变体尚未验证。".into());
    }
    let snapshots = collect_banks(&result.bank_events)?;
    attach_banks(&mut result, snapshots);
    result.alien_selection = selection::review(&result, evidence::parse(bytes, &details, &init));
    if result.players.len() != 12 {
        result.warnings.push(format!(
            "录像实际包含 {} 名非观战玩家；未补造十二人名额。",
            result.players.len()
        ));
    }
    Ok(result)
}

fn number(value: &Value, key: &str) -> Result<u32, String> {
    optional_number(value, key).ok_or_else(|| format!("录像字段 {key} 缺失或不是有效整数。"))
}

fn optional_number(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

fn unique<T>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.next().is_none().then_some(first)
}

fn byte_array(value: &Value) -> Result<Vec<u8>, String> {
    value
        .as_array()
        .ok_or("录像文本或二进制字段不是字节数组。")?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or_else(|| "录像字节数组包含无效值。".to_owned())
        })
        .collect()
}

fn text(value: &Value, key: &str) -> Result<String, String> {
    let bytes = byte_array(
        value
            .get(key)
            .ok_or_else(|| format!("录像缺少字段 {key}。"))?,
    )?;
    String::from_utf8(bytes).map_err(|error| format!("录像字段 {key} 不是有效 UTF-8：{error}"))
}

fn bank_data(event: &Value, warnings: &mut Vec<String>) -> Result<(String, bool), String> {
    let bytes = byte_array(event.get("m_data").ok_or("存档事件缺少数据。")?)?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok((text, true)),
        Err(error) => {
            warnings.push(
                "此存档包含非 UTF-8 数据；结构视图以十六进制显示，原始字节仍保留于本次读取记录。"
                    .into(),
            );
            Ok((format!("[binary:{:02x?}]", error.as_bytes()), false))
        }
    }
}

fn clean_name(name: &str) -> String {
    name.replace("<sp/>", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn players(details: &Value, init: &Value) -> Result<Vec<ReplayPlayer>, String> {
    let list = details
        .get("m_playerList")
        .and_then(Value::as_array)
        .ok_or("录像缺少玩家列表。")?;
    let slots = init
        .pointer("/m_syncLobbyState/m_lobbyState/m_slots")
        .and_then(Value::as_array)
        .ok_or("录像缺少大厅槽位映射。")?;
    let mut players = Vec::new();
    for player in list {
        if number(player, "m_observe")? != 0 {
            continue;
        }
        let toon = player.get("m_toon").ok_or("录像玩家缺少身份。")?;
        let handle = format!(
            "{}-S2-{}-{}",
            number(toon, "m_region")?,
            number(toon, "m_realm")?,
            number(toon, "m_id")?
        );
        let slot_id = optional_number(player, "m_workingSetSlotId");
        let user_id = unique(slots.iter().filter(|slot| {
            slot_id.is_some()
                && optional_number(slot, "m_workingSetSlotId") == slot_id
                && text(slot, "m_toonHandle").ok().as_deref() == Some(handle.as_str())
        }))
        .and_then(|slot| optional_number(slot, "m_userId"));
        let mut warnings = Vec::new();
        if user_id.is_none() {
            warnings.push("无法唯一关联大厅用户与玩家身份，不按列表顺序猜测。".into());
        }
        players.push(ReplayPlayer {
            slot_id,
            user_id,
            name: clean_name(&text(player, "m_name")?),
            toon_handle: handle,
            banks: Vec::new(),
            selected_bank: None,
            warnings,
        });
    }
    if players.len() > 32 {
        return Err("录像玩家数量异常。".into());
    }
    Ok(players)
}

struct BankBuilder {
    bank: ReplayBank,
    section: Option<String>,
    key: Option<String>,
}

impl BankBuilder {
    fn new(event: &Value) -> Result<Self, String> {
        Ok(Self {
            bank: ReplayBank {
                ambiguous_core: false,
                name: text(event, "m_name")?,
                gameloop: number(event, "_gameloop")?,
                end_gameloop: None,
                toon_handle: String::new(),
                signature: Vec::new(),
                sections: BTreeMap::new(),
                report: None,
                warnings: Vec::new(),
            },
            section: None,
            key: None,
        })
    }

    fn section(&mut self, event: &Value) -> Result<(), String> {
        let name = text(event, "m_name")?;
        self.bank.sections.entry(name.clone()).or_default();
        self.section = Some(name);
        self.key = None;
        Ok(())
    }

    fn key(&mut self, event: &Value) -> Result<(), String> {
        let name = text(event, "m_name")?;
        let section = self
            .section
            .as_ref()
            .ok_or("BankKey 事件之前没有 BankSection。")?;
        let keys = self
            .bank
            .sections
            .get_mut(section)
            .ok_or("存档节状态无效。")?;
        let (data, data_is_utf8) = bank_data(event, &mut self.bank.warnings)?;
        let key = ReplayKey {
            value_type: number(event, "m_type")?,
            data,
            data_is_utf8,
            values: Vec::new(),
        };
        if keys.insert(name.clone(), key).is_some() {
            if matches!(
                (section.as_str(), name.as_str()),
                ("Encrypted", "Bank") | ("FA", "BankVersion")
            ) {
                self.bank.ambiguous_core = true;
            }
            self.bank.warnings.push(format!(
                "重复键 {section}/{name}；结构视图显示后值，完整事件仍保留于本次读取记录。"
            ));
        }
        self.key = Some(name);
        Ok(())
    }

    fn value(&mut self, event: &Value) -> Result<(), String> {
        let (data, data_is_utf8) = bank_data(event, &mut self.bank.warnings)?;
        let key = self
            .section
            .as_ref()
            .and_then(|section| self.bank.sections.get_mut(section))
            .and_then(|keys| self.key.as_ref().and_then(|name| keys.get_mut(name)))
            .ok_or("BankValue 事件之前没有有效的 BankKey。")?;
        key.values.push(ReplayValue {
            name: text(event, "m_name")?,
            value_type: number(event, "m_type")?,
            data,
            data_is_utf8,
        });
        Ok(())
    }
}

fn collect_banks(events: &[Value]) -> Result<Vec<(u32, ReplayBank)>, String> {
    let mut active: BTreeMap<u32, BankBuilder> = BTreeMap::new();
    let mut banks = Vec::new();
    for event in events {
        let user = number(
            event.get("_userid").ok_or("录像事件缺少用户。")?,
            "m_userId",
        )?;
        let kind = event
            .get("_event")
            .and_then(Value::as_str)
            .ok_or("录像事件缺少名称。")?;
        if kind.ends_with("SBankFileEvent") {
            if let Some(mut previous) = active.insert(user, BankBuilder::new(event)?) {
                previous
                    .bank
                    .warnings
                    .push("同步未出现结束事件，不解码不完整快照。".into());
                banks.push((user, previous.bank));
            }
            continue;
        }
        if kind.ends_with("SBankSignatureEvent") {
            let mut current = active
                .remove(&user)
                .ok_or("BankSignature 事件之前没有 BankFile。")?;
            current.bank.toon_handle = text(event, "m_toonHandle")?;
            current.bank.signature = byte_array(
                event
                    .get("m_signature")
                    .ok_or("录像存档缺少签名事件字段。")?,
            )?;
            current.bank.end_gameloop = Some(number(event, "_gameloop")?);
            banks.push((user, current.bank));
            continue;
        }
        let current = active.get_mut(&user).ok_or("存档事件之前没有 BankFile。")?;
        if kind.ends_with("SBankSectionEvent") {
            current.section(event)?;
        } else if kind.ends_with("SBankKeyEvent") {
            current.key(event)?;
        } else if kind.ends_with("SBankValueEvent") {
            current.value(event)?;
        } else {
            return Err(format!("不支持的存档事件 {kind}。"));
        }
    }
    for (user, mut current) in active {
        current
            .bank
            .warnings
            .push("同步未出现结束事件，不解码不完整快照。".into());
        banks.push((user, current.bank));
    }
    Ok(banks)
}

fn attach_banks(result: &mut ReplayReport, banks: Vec<(u32, ReplayBank)>) {
    for (user, mut snapshot) in banks {
        let player = unique(
            result
                .players
                .iter_mut()
                .filter(|p| p.user_id == Some(user)),
        );
        if let Some(player) = player
            .filter(|p| snapshot.toon_handle.is_empty() || snapshot.toon_handle == p.toon_handle)
        {
            if snapshot.end_gameloop.is_some() && matches!(snapshot.name.as_str(), "RR" | "XX") {
                match decode_bank(&snapshot, player, &result.path) {
                    Ok(report) => snapshot.report = Some(report),
                    Err(error) => snapshot.warnings.push(error),
                }
            }
            player.banks.push(snapshot);
        } else {
            snapshot.warnings.push(format!(
                "用户 {user} 的槽位/句柄无法唯一匹配，未分配给其他玩家。"
            ));
            result.unmapped_banks.push(snapshot);
        }
    }
    for player in &mut result.players {
        // Mirror the map's RR/XX choice by section presence, not by decoding
        // success: never hide a corrupt main bank behind a readable backup.
        let primary = player.banks.iter().rposition(|b| b.name == "RR");
        let backup = player
            .banks
            .iter()
            .rposition(|b| b.name == "XX" && b.sections.contains_key("Encrypted"));
        player.selected_bank = primary
            .filter(|index| {
                player
                    .banks
                    .get(*index)
                    .is_some_and(|b| b.sections.contains_key("Encrypted"))
            })
            .or(backup)
            .or(primary);
        if player.banks.is_empty() {
            player
                .warnings
                .push("录像没有此玩家的存档同步记录。".into());
        }
        if !player.banks.iter().any(|b| b.report.is_some()) {
            player
                .warnings
                .push("未记录可解码的核心载荷，历史统计未知。".into());
        }
    }
    if !result.unmapped_banks.is_empty() {
        result.warnings.push(format!(
            "{} 份快照无法确认玩家归属，已单独保留。",
            result.unmapped_banks.len()
        ));
    }
}

fn decode_bank(
    snapshot: &ReplayBank,
    player: &ReplayPlayer,
    path: &Path,
) -> Result<BankReport, String> {
    let key = snapshot
        .sections
        .get("Encrypted")
        .and_then(|s| s.get("Bank"))
        .ok_or("未记录 Encrypted/Bank 核心载荷；不能从缺失数据恢复历史统计。")?;
    let payload = match key.value_type {
        3 if key.values.is_empty() && key.data_is_utf8 => key.data.as_str(),
        7 => unique(
            key.values
                .iter()
                .filter(|v| v.name == "Value" && v.value_type == 3 && v.data_is_utf8),
        )
        .ok_or("Encrypted/Bank 没有唯一的字符串 Value。")?
        .data
        .as_str(),
        kind => return Err(format!("Encrypted/Bank 类型 {kind} 暂不支持。")),
    };
    if payload.is_empty() {
        return Err("核心载荷为空，未按零值存档解码。".into());
    }
    let version = snapshot
        .sections
        .get("FA")
        .and_then(|s| s.get("BankVersion"))
        .map_or(Ok(0), |key| {
            if key.value_type != 2 {
                return Err("BankVersion 不是整数类型。".to_owned());
            }
            key.data
                .parse::<u32>()
                .map_err(|error| format!("BankVersion 无效：{error}"))
        })?;
    let fields = bank::decode(payload, version, Some(&player.toon_handle))?;
    let mut note = format!(
        "录像只读快照：{}，同步循环 {}；不是结算存档。",
        snapshot.name, snapshot.gameloop
    );
    let expected = player
        .toon_handle
        .rsplit('-')
        .next()
        .and_then(|v| v.parse::<u64>().ok());
    if expected != fields.get("player_id").copied() {
        note.push_str(" 载荷玩家 ID 与录像句柄不一致，地图可能拒绝或重置这些数据。");
    }
    Ok(BankReport {
        account_id: player.name.clone(),
        path: path.to_path_buf(),
        server_id: player
            .toon_handle
            .split('-')
            .next()
            .and_then(|v| v.parse().ok()),
        bank_version: version,
        player_name: snapshot.player_name().map(str::to_owned),
        fields,
        decode_note: note,
        signature_present: !snapshot.signature.is_empty(),
        source_text: String::new(),
    })
}
