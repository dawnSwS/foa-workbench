//! Replay facts for the exact audited map revision; no selection simulation.
//!
//! MapScript.galaxy (cache handle 6c6561…51a9a): role assignment waits 0.1875
//! game seconds (loop 3); alien selection waits 0.25 seconds (loop 4).
//! Later upgrades are deliberately not treated as starting identities.

use serde_json::Value;

use super::{mpq, protocol};

pub const SOURCE_HASH: &str = "6c6561c69495e5d31a180d7ed55cefeb9f9473ea55bc591addfb1f34f2551a9a";
const MAP_NAMESPACE: u32 = 178_950;
const SOURCE_DIGEST: &[u8] = &[
    0x6c, 0x65, 0x61, 0xc6, 0x94, 0x95, 0xe5, 0xd3, 0x1a, 0x18, 0x0d, 0x7e, 0xd5, 0x5c, 0xef, 0xeb,
    0x9f, 0x94, 0x73, 0xea, 0x55, 0xbc, 0x59, 0x1a, 0xdd, 0xfb, 0x1f, 0x34, 0xf2, 0x55, 0x1a, 0x9a,
];
const ROLE_LOOP: u32 = 3;
const SELECTION_LOOP: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignupChoice {
    Neither,
    Host,
    Successor,
    Both,
}

#[derive(Debug, Clone)]
pub struct UpgradeFact {
    pub gameloop: u32,
    pub player_id: u32,
    pub upgrade: String,
    /// Tracker `m_count` is a change, not the final upgrade level.
    pub delta: i32,
}

#[derive(Debug, Clone)]
pub struct PlayerEvidence {
    /// Galaxy/Tracker player ID (1-based); never derived from list order.
    pub player_id: u32,
    pub user_id: u32,
    /// Physical index in the lobby slots array; Tracker `m_slotId`.
    pub slot_id: u32,
    /// Details/initData `m_workingSetSlotId`; may differ from `slot_id`.
    pub working_set_slot_id: u32,
    pub toon_handle: String,
    pub signup: Option<SignupChoice>,
    pub signup_raw: Option<String>,
    pub role: Option<u32>,
    pub role_event: Option<UpgradeFact>,
}

#[derive(Debug, Clone)]
pub struct ReplayEvidence {
    pub source_hash: &'static str,
    pub map_namespace: u32,
    pub pve: Option<bool>,
    pub players: Vec<PlayerEvidence>,
    pub opening_host: Option<UpgradeFact>,
    pub opening_successor: Option<UpgradeFact>,
    pub warnings: Vec<String>,
}

struct Attribute {
    namespace: u32,
    id: u32,
    scope: u8,
    value: [u8; 4],
}

struct Attributes {
    namespace: u32,
    entries: Vec<Attribute>,
}

pub fn parse(bytes: &[u8], details: &Value, init: &Value) -> Result<ReplayEvidence, String> {
    let attributes = attributes(&mpq::extract(bytes, "replay.attributes.events")?)?;
    check_map(details, init, attributes.namespace)?;
    let tracker = protocol::tracker_events(&mpq::extract(bytes, "replay.tracker.events")?)?;
    facts(details, init, &attributes, &tracker)
}

fn check_map(details: &Value, init: &Value, namespace: u32) -> Result<(), String> {
    if namespace != MAP_NAMESPACE {
        return Err(format!(
            "录像地图命名空间为 {namespace}，当前选人规则仅核对过 {MAP_NAMESPACE}。"
        ));
    }
    let detail_handles = details
        .get("m_cacheHandles")
        .and_then(Value::as_array)
        .ok_or("录像缺少 details 地图缓存句柄，不能验证规则版本。")?;
    let init_handles = init
        .pointer("/m_syncLobbyState/m_gameDescription/m_cacheHandles")
        .and_then(Value::as_array)
        .ok_or("录像缺少 initData 地图缓存句柄，不能验证规则版本。")?;
    if detail_handles != init_handles {
        return Err("录像 details 与 initData 的地图缓存链不一致，不能验证规则版本。".into());
    }
    if !has_source_hash(detail_handles)? || !has_source_hash(init_handles)? {
        return Err("录像末尾主地图缓存哈希不是已审查版本，不能套用本版选人规则。".into());
    }
    Ok(())
}

fn has_source_hash(handles: &[Value]) -> Result<bool, String> {
    // The last cache handle identifies the primary map; merely finding this
    // revision among its dependencies does not authorize its selection rules.
    let Some(handle) = handles.last() else {
        return Ok(false);
    };
    let bytes = byte_values(handle)?;
    Ok(bytes.len() == 40
        && bytes.get(..4) == Some(b"s2ma")
        && bytes.get(8..) == Some(SOURCE_DIGEST))
}

fn attributes(bytes: &[u8]) -> Result<Attributes, String> {
    if bytes.first() != Some(&0) {
        return Err("录像属性流缺失或使用未支持的来源格式。".into());
    }
    let namespace = little_u32(bytes, 1)?;
    let count = usize::try_from(little_u32(bytes, 5)?).map_err(|error| error.to_string())?;
    if count > 100_000 {
        return Err("录像属性数量超过限制。".into());
    }
    let expected = count
        .checked_mul(13)
        .and_then(|n| n.checked_add(9))
        .ok_or("录像属性长度溢出。")?;
    if bytes.len() != expected {
        return Err("录像属性流长度与声明数量不一致。".into());
    }
    let mut entries = Vec::with_capacity(count);
    for chunk in bytes
        .get(9..)
        .ok_or("录像属性头截断。")?
        .as_chunks::<13>()
        .0
    {
        let mut raw: [u8; 4] = chunk
            .get(9..13)
            .ok_or("录像属性值截断。")?
            .try_into()
            .map_err(|_| "录像属性值长度无效。")?;
        raw.reverse();
        entries.push(Attribute {
            namespace: little_u32(chunk, 0)?,
            id: little_u32(chunk, 4)?,
            scope: *chunk.get(8).ok_or("录像属性scope缺失。")?,
            value: raw,
        });
    }
    Ok(Attributes { namespace, entries })
}

fn little_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("录像属性偏移溢出。")?;
    let data: [u8; 4] = bytes
        .get(offset..end)
        .ok_or("录像属性数据截断。")?
        .try_into()
        .map_err(|_| "录像属性整数无效。")?;
    Ok(u32::from_le_bytes(data))
}

fn attribute_value(attributes: &Attributes, scope: u32, id: u32) -> Result<Option<String>, String> {
    let mut matching = attributes.entries.iter().filter(|entry| {
        entry.namespace == MAP_NAMESPACE && u32::from(entry.scope) == scope && entry.id == id
    });
    let value = matching
        .next()
        .map(|entry| {
            std::str::from_utf8(&entry.value)
                .map(|text| text.trim_matches('\0').to_owned())
                .map_err(|error| format!("地图属性值不是 UTF-8：{error}"))
        })
        .transpose()?;
    if matching.next().is_some() {
        return Err(format!(
            "地图属性 {id} / scope {scope} 重复，无法确定报名信息。"
        ));
    }
    Ok(value)
}

fn facts(
    details: &Value,
    init: &Value,
    attributes: &Attributes,
    tracker: &[Value],
) -> Result<ReplayEvidence, String> {
    let mut warnings = Vec::new();
    let pve = match attribute_value(attributes, 16, 7)?.as_deref() {
        Some("0001") => Some(false),
        Some("0002") => Some(true),
        _ => {
            warnings.push("未确认本局 PvE 属性，不据此假设选人流程会运行。".into());
            None
        }
    };
    let mut players = player_mapping(details, init, tracker)?;
    let upgrades = opening_upgrades(tracker)?;
    for player in &mut players {
        player.signup_raw = attribute_value(attributes, player.player_id, 2)?;
        player.signup = match player.signup_raw.as_deref() {
            Some("0001") => Some(SignupChoice::Neither),
            Some("0002") => Some(SignupChoice::Host),
            Some("0003") => Some(SignupChoice::Successor),
            Some("0004") => Some(SignupChoice::Both),
            _ => {
                warnings.push(format!("玩家 {} 的报名属性缺失或未知。", player.player_id));
                None
            }
        };
        let roles: Vec<_> = upgrades
            .iter()
            .filter(|event| {
                event.player_id == player.player_id
                    && event.gameloop == ROLE_LOOP
                    && role_id(&event.upgrade).is_some()
            })
            .collect();
        if roles.len() == 1 && roles.first().is_some_and(|event| event.delta == 1) {
            if let Some(event) = roles.first() {
                player.role = role_id(&event.upgrade);
                player.role_event = Some((*event).clone());
            }
        } else {
            warnings.push(format!(
                "玩家 {} 在 loop 3 没有唯一的初始职位升级证据。",
                player.player_id
            ));
        }
    }
    let opening_host = unique_opening(&upgrades, "AlienHost", &players, &mut warnings);
    let opening_successor = unique_opening(&upgrades, "OriginalSuccessor", &players, &mut warnings);
    if pve == Some(true) && (opening_host.is_some() || opening_successor.is_some()) {
        warnings.push("PvE 属性与初始异形升级事实冲突，不能据此复盘选人结果。".into());
    }
    Ok(ReplayEvidence {
        source_hash: SOURCE_HASH,
        map_namespace: attributes.namespace,
        pve,
        players,
        opening_host,
        opening_successor,
        warnings,
    })
}

fn player_mapping(
    details: &Value,
    init: &Value,
    tracker: &[Value],
) -> Result<Vec<PlayerEvidence>, String> {
    let participants = details
        .get("m_playerList")
        .and_then(Value::as_array)
        .ok_or("缺少录像玩家。")?;
    let slots = init
        .pointer("/m_syncLobbyState/m_lobbyState/m_slots")
        .and_then(Value::as_array)
        .ok_or("缺少大厅槽位。")?;
    let mut players = Vec::new();
    for participant in participants {
        if field_u32(participant, "m_observe")? != 0 {
            continue;
        }
        let toon = participant.get("m_toon").ok_or("缺少玩家句柄。")?;
        let handle = format!(
            "{}-S2-{}-{}",
            field_u32(toon, "m_region")?,
            field_u32(toon, "m_realm")?,
            field_u32(toon, "m_id")?
        );
        let working_slot = field_u32(participant, "m_workingSetSlotId")?;
        let matching: Vec<_> = slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                field_u32(slot, "m_workingSetSlotId").ok() == Some(working_slot)
                    && byte_text(slot, "m_toonHandle").ok().as_deref() == Some(handle.as_str())
            })
            .collect();
        if matching.len() != 1 {
            return Err(format!("玩家 {handle} 的大厅身份映射不唯一。"));
        }
        let (slot_index, slot) = matching.first().ok_or("玩家大厅槽位缺失。")?;
        let user_id = field_u32(slot, "m_userId")?;
        let slot_id = u32::try_from(*slot_index).map_err(|error| error.to_string())?;
        let setups: Vec<_> = tracker
            .iter()
            .filter(|event| {
                event.get("_eventid").and_then(Value::as_u64) == Some(9)
                    && field_u32(event, "_gameloop").ok() == Some(0)
                    && field_u32(event, "m_userId").ok() == Some(user_id)
                    && field_u32(event, "m_slotId").ok() == Some(slot_id)
            })
            .collect();
        if setups.len() != 1 {
            return Err(format!("玩家 {handle} 缺少唯一 Tracker 初始化映射。"));
        }
        let setup = setups.first().ok_or("Tracker 玩家初始化缺失。")?;
        let player_id = field_u32(setup, "m_playerId")?;
        if !(1..=12).contains(&player_id)
            || players
                .iter()
                .any(|p: &PlayerEvidence| p.player_id == player_id)
        {
            return Err("Tracker 玩家 ID 超界或重复，不能推断游戏内编号。".into());
        }
        players.push(PlayerEvidence {
            player_id,
            user_id,
            slot_id,
            working_set_slot_id: working_slot,
            toon_handle: handle,
            signup: None,
            signup_raw: None,
            role: None,
            role_event: None,
        });
    }
    Ok(players)
}

fn opening_upgrades(tracker: &[Value]) -> Result<Vec<UpgradeFact>, String> {
    let mut events = Vec::new();
    for event in tracker {
        if event.get("_eventid").and_then(Value::as_u64) != Some(5) {
            continue;
        }
        let gameloop = field_u32(event, "_gameloop")?;
        if !matches!(gameloop, ROLE_LOOP | SELECTION_LOOP) {
            continue;
        }
        let upgrade = byte_text(event, "m_upgradeTypeName")?;
        if !matches!(upgrade.as_str(), "AlienHost" | "OriginalSuccessor")
            && role_id(&upgrade).is_none()
        {
            continue;
        }
        let delta = event
            .get("m_count")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or("升级变化量无效。")?;
        events.push(UpgradeFact {
            gameloop,
            player_id: field_u32(event, "m_playerId")?,
            upgrade,
            delta,
        });
    }
    Ok(events)
}

fn unique_opening(
    upgrades: &[UpgradeFact],
    name: &str,
    players: &[PlayerEvidence],
    warnings: &mut Vec<String>,
) -> Option<UpgradeFact> {
    let matching: Vec<_> = upgrades
        .iter()
        .filter(|event| event.gameloop == SELECTION_LOOP && event.upgrade == name)
        .collect();
    if matching.len() == 1
        && matching.first().is_some_and(|event| {
            event.delta == 1
                && players
                    .iter()
                    .any(|player| player.player_id == event.player_id)
        })
    {
        matching.first().map(|event| (*event).clone())
    } else {
        warnings.push(format!(
            "loop 4 未找到唯一的 {name} +1，未用后期变化补作初始结果。"
        ));
        None
    }
}

fn role_id(name: &str) -> Option<u32> {
    match name {
        "1Captain" => Some(1),
        "2Lieutenant" => Some(2),
        "3WeaponsOfficer" => Some(3),
        "4MechanicalOfficer" => Some(4),
        "5Enginner" => Some(5),
        "6TechnologicalOfficer" => Some(6),
        "7Doctor" => Some(7),
        "8SecurityChief" => Some(8),
        "9Pilot" => Some(9),
        "10Biologist" => Some(10),
        "11Private" | "12Private" => Some(11),
        _ => None,
    }
}

fn field_u32(value: &Value, name: &str) -> Result<u32, String> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .ok_or_else(|| format!("录像证据字段 {name} 缺失或无效。"))
}

fn byte_values(value: &Value) -> Result<Vec<u8>, String> {
    value
        .as_array()
        .ok_or("录像证据字段不是字节数组。")?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|number| u8::try_from(number).ok())
                .ok_or_else(|| "录像证据字节无效。".into())
        })
        .collect()
}

fn byte_text(value: &Value, name: &str) -> Result<String, String> {
    let bytes = byte_values(
        value
            .get(name)
            .ok_or_else(|| format!("缺少录像证据字段 {name}。"))?,
    )?;
    String::from_utf8(bytes).map_err(|error| format!("录像证据文本无效：{error}"))
}
