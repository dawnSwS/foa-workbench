//! Join early replay snapshots with the exact map profile's opening facts.
//! Native arithmetic boundaries are enclosed; Galaxy RNG values are not invented.
use std::collections::BTreeMap;

use crate::alien_selection::{
    self, ExclusionReason, HostChoice, PlayerInput, ScoreRange, SelectionAnalysis, SelectionInput,
};
use crate::model::FIELDS;

use super::evidence::{PlayerEvidence, ReplayEvidence, SignupChoice};
use super::opening::{opening_stats, OpeningStats};
use super::{unique, ReplayPlayer, ReplayReport};

#[derive(Default)]
pub struct AlienSelectionReview {
    pub analysis: Option<SelectionAnalysis>,
    pub observed: Vec<ObservedAlien>,
    pub player_names: BTreeMap<u32, String>,
    pub notes: Vec<String>,
}

pub struct ObservedAlien {
    pub player_id: u32,
    pub role: &'static str,
    pub gameloop: u32,
}

pub(super) fn review(
    replay: &ReplayReport,
    evidence: Result<ReplayEvidence, String>,
) -> AlienSelectionReview {
    let evidence = match evidence {
        Ok(evidence) => evidence,
        Err(error) => {
            return AlienSelectionReview {
                notes: vec![format!(
                    "无法启用这份录像的选人复盘：{error}。存档查看仍可使用。"
                )],
                ..AlienSelectionReview::default()
            };
        }
    };
    let mut result = AlienSelectionReview {
        notes: evidence.warnings.clone(),
        ..AlienSelectionReview::default()
    };
    result.notes.push(format!(
        "已匹配审查过的地图内容 {}，大厅属性命名空间 {}。",
        evidence.source_hash, evidence.map_namespace
    ));
    result.notes.push(
        "使用加载阶段已完成同步的 RR/XX；无载荷时依据完整预载证据复原地图初值。切换详情快照不会改变复盘输入。".into(),
    );
    let mut players = Vec::new();
    let mut missing = Vec::new();
    let mut roster_complete = evidence.players.len() == replay.players.len();
    let single = replay.players.len() < 4 && evidence.pve == Some(false);
    for fact in &evidence.players {
        let Some(player) = mapped_player(replay, fact) else {
            roster_complete = false;
            result.notes.push(format!(
                "游戏玩家 {} 无法唯一映射到存档，未猜测身份。",
                fact.player_id
            ));
            continue;
        };
        result
            .player_names
            .insert(fact.player_id, player.name.clone());
        if fact.slot_id != fact.working_set_slot_id {
            result.notes.push(format!(
                "{} 的大厅槽位与工作集编号不同，已通过录像身份映射关联。",
                player.name
            ));
        }
        let bank = opening_stats(replay, player);
        match bank {
            OpeningStats::Unknown(reason) => missing.push(format!("{}（{reason}）", player.name)),
            OpeningStats::Defaults => result.notes.push(format!(
                "{}：依据开局同步证据确认选定 RR/XX 无核心载荷；按地图初值复原选人输入（累计场次初值 0、本局处理后 {}、HMMR/AMMR 归一化为 100），不写入历史存档表。",
                player.name, u32::from(!single)
            )),
            OpeningStats::Loaded(report) if report.bank_version < 4 => result.notes.push(format!(
                "{}：存档版本 {} 尚未引入的字段按地图加载分支使用初值 0；该版本本应存在却未读取到的字段仍保持未知。",
                player.name, report.bank_version
            )),
            OpeningStats::Loaded(_) => {},
        }
        let banned = whole_game_banned(bank, &player.toon_handle);
        if banned != Some(false) {
            roster_complete = false;
        }
        players.push(player_input(fact, player, bank, single));
    }
    for (fact, role) in [
        (evidence.opening_host.as_ref(), "最初的异形主宰"),
        (evidence.opening_successor.as_ref(), "最初的异形继任者"),
    ] {
        if let Some(fact) = fact.filter(|fact| result.player_names.contains_key(&fact.player_id)) {
            result.observed.push(ObservedAlien {
                player_id: fact.player_id,
                role,
                gameloop: fact.gameloop,
            });
        }
    }
    let input = SelectionInput {
        players,
        roster_complete,
        enabled: evidence.pve.map(|pve| !pve),
        // This exact map revision never assigns either requirement local.
        // Galaxy initializes both integers to zero, so 0 < 0 is false.
        requirement_penalty: Some(false),
        first_host: evidence
            .opening_host
            .as_ref()
            .map(|fact| HostChoice::Observed(fact.player_id)),
        estimate_fixed_arithmetic: true,
    };
    let mut analysis = alien_selection::analyze(&input);
    explain_exclusions(&mut analysis, replay, &evidence, single);
    analysis.reasons.insert(0, "按地图公式传播整数区间，并比较所有候选的胜负门槛；区间是可能值的包络，不是录像记录的实际分数，也不是当选概率。".into());
    analysis.reasons.push(
        "本地图的两个 requirement 局部整数均未赋值，默认值为 0，额外扣 200 分条件恒假。".into(),
    );
    if !missing.is_empty() {
        analysis.reasons.push(format!(
            "无法确定的开局输入：{}。这些数值保持未知，不补零。",
            missing.join("、")
        ));
    }
    if !roster_complete {
        result.notes.push(
            "参与者映射、存档或整局封禁状态存在未知；不假设录像名单全部按普通路径参与平均值计算。"
                .into(),
        );
    }
    result.notes.push("模型按普通开局处理，不包含选人前管理员命令的临时干预；最初的异形继任者胜场未写入此版 Bank，采用开局初值 0。".into());
    result.analysis = Some(analysis);
    result
}

fn mapped_player<'a>(replay: &'a ReplayReport, fact: &PlayerEvidence) -> Option<&'a ReplayPlayer> {
    unique(replay.players.iter().filter(|player| {
        player.user_id == Some(fact.user_id)
            && player.slot_id == Some(fact.working_set_slot_id)
            && player.toon_handle == fact.toon_handle
    }))
}

fn explain_exclusions(
    analysis: &mut SelectionAnalysis,
    replay: &ReplayReport,
    evidence: &ReplayEvidence,
    single: bool,
) {
    for row in analysis
        .rounds
        .iter_mut()
        .flat_map(|round| &mut round.players)
    {
        let Some(fact) = unique(
            evidence
                .players
                .iter()
                .filter(|fact| fact.player_id == row.id),
        ) else {
            continue;
        };
        let Some(player) = mapped_player(replay, fact) else {
            continue;
        };
        let bank = opening_stats(replay, player);
        for exclusion in &mut row.exclusions {
            exclusion.detail = match &exclusion.reason {
                ExclusionReason::NewPlayer => new_player_detail(bank),
                ExclusionReason::NewRole => new_role_detail(bank, fact.role, single),
                ExclusionReason::HostBan => host_ban_detail(bank, &player.toon_handle, single),
                ExclusionReason::SpecialRole(host) => {
                    special_role_detail(replay, evidence, fact.role, *host)
                }
                ExclusionReason::AlreadyAlien | ExclusionReason::SelectedHost => continue,
            };
        }
    }
}

fn role_name(role: Option<u32>) -> String {
    let Some(role) = role else {
        return "职位未记录".into();
    };
    let key = format!("lv_statsGamesPlayedAsRole[{role}]");
    FIELDS.iter().find(|field| field.key == key).map_or_else(
        || format!("未知职位（{role}）"),
        |field| field.label.trim_end_matches("出场次数").to_owned(),
    )
}

fn new_player_detail(bank: OpeningStats<'_>) -> String {
    metric(bank, "lv_statsGamesPlayed").map_or_else(
        || "历史累计场次未记录；新玩家保护门槛为计入本局后少于 10 场。".into(),
        |games| {
            format!(
                "历史累计 {games} 场，计入本局后为 {} 场，低于 10 场的新玩家保护门槛。",
                games.saturating_add(1)
            )
        },
    )
}

fn new_role_detail(bank: OpeningStats<'_>, role: Option<u32>, single: bool) -> String {
    let name = role_name(role);
    let played = role.and_then(|role| metric(bank, &format!("lv_statsGamesPlayedAsRole[{role}]")));
    let detail = played.map_or_else(
        || format!("当局职位：{name}；该职位历史次数未记录；新职业保护门槛为计入本局后少于 4 次。"),
        |played| format!("当局职位：{name}；历史该职位 {played} 次，计入本局后为 {} 次，低于 4 次的新职业保护门槛。", played.saturating_add(1)),
    );
    let total = metric(bank, "lv_statsGamesPlayed").map_or_else(
        || "历史累计场次未记录。".into(),
        |games| {
            if single {
                return format!("历史累计 {games} 场；单人模式不增加累计场次，仍为 {games} 场，且豁免新玩家保护。");
            }
            let loaded = games.saturating_add(1);
            let distinction = if loaded >= 10 {
                "，不是累计不足 10 场的新玩家保护"
            } else {
                "；本行原因依据该职位的出场次数"
            };
            format!("历史累计 {games} 场，计入本局后为 {loaded} 场{distinction}。")
        },
    );
    format!("{detail}{total}")
}

fn special_role_detail(
    replay: &ReplayReport,
    evidence: &ReplayEvidence,
    role: Option<u32>,
    host: u32,
) -> String {
    let host_fact = unique(
        evidence
            .players
            .iter()
            .filter(|fact| fact.player_id == host),
    );
    let host_name = host_fact
        .and_then(|fact| mapped_player(replay, fact))
        .map_or_else(
            || format!("玩家 {host}（姓名未记录）"),
            |player| player.name.clone(),
        );
    format!(
        "首轮异形主宰为 {host_name}（{}）；本人当局职位为 {}。舰长、副官、工程师、安全主管属于关键职位；首轮异形主宰占用关键职位时，第二轮排除其他关键职位。",
        role_name(host_fact.and_then(|fact| fact.role)),
        role_name(role),
    )
}

fn metric(bank: OpeningStats<'_>, key: &str) -> Option<u64> {
    bank.metric(key)
}
fn flag(bank: OpeningStats<'_>, key: &str) -> Option<bool> {
    metric(bank, key).map(|value| value != 0)
}

fn new_player(bank: OpeningStats<'_>, handle: &str, single: bool) -> Option<bool> {
    if single
        || matches!(
            handle,
            "1-S2-1-2984665"
                | "1-S2-1-10778621"
                | "2-S2-1-8794834"
                | "2-S2-1-8907564"
                | "1-S2-1-9933296"
        )
    {
        return Some(false);
    }
    metric(bank, "lv_statsGamesPlayed").map(|games| games.saturating_add(1) < 10)
}

fn new_role(bank: OpeningStats<'_>, role: Option<u32>, new: Option<bool>) -> Option<bool> {
    if new == Some(true) {
        return Some(false);
    }
    let played =
        role.and_then(|role| metric(bank, &format!("lv_statsGamesPlayedAsRole[{role}]")))?;
    if played.saturating_add(1) >= 4 {
        Some(false)
    } else {
        new.map(|new| !new)
    }
}

fn any(flags: &[Option<bool>]) -> Option<bool> {
    if flags.contains(&Some(true)) {
        Some(true)
    } else if flags.iter().all(Option::is_some) {
        Some(false)
    } else {
        None
    }
}

fn host_banned(bank: OpeningStats<'_>, handle: &str, single: bool) -> Option<bool> {
    any(&host_ban_checks(bank, handle, single))
}

fn host_ban_counters(bank: OpeningStats<'_>, single: bool) -> [Option<(u64, u64)>; 2] {
    [
        ("lv_autoModTrollAlien", if single { 0 } else { 5 }),
        ("lv_autoModTemphostbangames", u64::from(!single)),
    ]
    .map(|(key, reduction)| metric(bank, key).map(|value| (value, value.saturating_sub(reduction))))
}

fn host_ban_checks(bank: OpeningStats<'_>, handle: &str, single: bool) -> [Option<bool>; 4] {
    let hardcoded = alien_selection::hardcoded_host_ban(handle);
    let [alien, temporary] = host_ban_counters(bank, single);
    let marked = if handle == "2-S2-1-4517637" {
        Some(false)
    } else {
        flag(bank, "lv_modAlienBan")
    };
    [
        Some(hardcoded),
        alien.map(|(_, value)| value > 100),
        temporary.map(|(_, value)| value > 0),
        marked,
    ]
}

fn host_ban_detail(bank: OpeningStats<'_>, handle: &str, single: bool) -> String {
    let [hardcoded, alien, temporary, marked] = host_ban_checks(bank, handle, single);
    let [alien_count, temporary_count] = host_ban_counters(bank, single);
    let adjustment = if single {
        "单人模式不递减，仍为"
    } else {
        "开局递减后为"
    };
    let mut causes = Vec::new();
    if hardcoded == Some(true) {
        causes.push(format!("句柄 {handle} 位于地图脚本的异形主宰禁选名单"));
    }
    if let Some((before, after)) = alien_count.filter(|_| alien == Some(true)) {
        causes.push(format!(
            "异形违规计数 {before}，{adjustment} {after}，大于 100"
        ));
    }
    if let Some((before, after)) = temporary_count.filter(|_| temporary == Some(true)) {
        causes.push(format!(
            "临时异形主宰禁选场次 {before}，{adjustment} {after}，大于 0"
        ));
    }
    if let Some(value) = metric(bank, "lv_modAlienBan").filter(|_| marked == Some(true)) {
        causes.push(format!("AlienBan（禁止竞选异形）标记为 {value}"));
    }
    if causes.is_empty() {
        "异形主宰禁选的具体依据未记录。".into()
    } else {
        format!("异形主宰禁选依据：{}。", causes.join("；"))
    }
}

fn whole_game_banned(bank: OpeningStats<'_>, handle: &str) -> Option<bool> {
    // Filled from the audited hard-coded ban predicate; uncertain membership
    // must never silently alter the room-average denominator.
    let hardcoded = alien_selection::hardcoded_game_ban(handle);
    any(&[Some(hardcoded), flag(bank, "lv_modBan")])
}

fn normalized_mmr(bank: OpeningStats<'_>, key: &str, single: bool) -> Option<ScoreRange> {
    let reset = if single {
        false
    } else {
        flag(bank, "lv_modTKScoreReset")?
    };
    let stored = if reset {
        100
    } else {
        i32::try_from(metric(bank, key)?).ok()?
    };
    regress_mmr(stored)
}

/// Enclose the audited `stored + FixedToInt((1000-stored) * 0.05)`.
/// The Q12 literals adjacent to 0.05 are 204/4096 and 205/4096.
/// Floor/ceiling endpoints include directed conversion boundaries instead of
/// asserting that a third-party VM profile was verified against this client.
fn regress_mmr(stored: i32) -> Option<ScoreRange> {
    if !(0..=100_000).contains(&stored) {
        return None;
    }
    if stored == 0 {
        return Some(ScoreRange { min: 100, max: 100 });
    }
    let delta = 1000_i64 - i64::from(stored);
    let lower_numerator = (delta * 204).min(delta * 205);
    let upper_numerator = (delta * 204).max(delta * 205);
    let lower = lower_numerator.div_euclid(4096);
    let upper = -(-upper_numerator).div_euclid(4096);
    Some(ScoreRange {
        min: stored.checked_add(i32::try_from(lower).ok()?)?,
        max: stored.checked_add(i32::try_from(upper).ok()?)?,
    })
}

fn player_input(
    fact: &PlayerEvidence,
    player: &ReplayPlayer,
    bank: OpeningStats<'_>,
    single: bool,
) -> PlayerInput {
    let new = new_player(bank, &player.toon_handle, single);
    let volunteer = match fact.signup {
        Some(SignupChoice::Neither) => [Some(false), Some(false)],
        Some(SignupChoice::Host) => [Some(true), Some(false)],
        Some(SignupChoice::Successor) => [Some(false), Some(true)],
        Some(SignupChoice::Both) => [Some(true), Some(true)],
        None => [None, None],
    };
    PlayerInput {
        id: fact.player_id,
        effective_hmmr: normalized_mmr(bank, "lv_alienSelectionHMMR", single),
        effective_ammr: normalized_mmr(bank, "lv_alienSelectionAMMR", single),
        host_wins: metric(bank, "lv_statsGamesWonHost").and_then(|value| u32::try_from(value).ok()),
        successor_wins: Some(0),
        new_player: new,
        new_role: new_role(bank, fact.role, new),
        host_banned: host_banned(bank, &player.toon_handle, single),
        already_alien: Some(false),
        volunteer,
        deprioritized: flag(bank, "lv_flagSpawnDepriortized"),
        left_last_game: flag(bank, "lv_modLeftLastGame"),
        special_role: fact.role.map(|role| matches!(role, 1 | 2 | 5 | 8)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{normalized_mmr, regress_mmr, OpeningStats};
    use crate::alien_selection::ScoreRange;
    use crate::model::BankReport;

    #[test]
    fn zero_and_equilibrium_have_exact_normalization() {
        assert_eq!(regress_mmr(0), Some(ScoreRange { min: 100, max: 100 }));
        assert_eq!(
            regress_mmr(1000),
            Some(ScoreRange {
                min: 1000,
                max: 1000
            })
        );
        assert_eq!(regress_mmr(-1), None);
        assert_eq!(regress_mmr(i32::MAX), None);
    }

    #[test]
    fn normalization_encloses_adjacent_q12_profiles() -> Result<(), String> {
        for stored in 1..=100_000 {
            let range = regress_mmr(stored).ok_or("Missing normalization range")?;
            let delta = 1000_i64 - i64::from(stored);
            for coefficient in [204, 205] {
                let numerator = delta * coefficient;
                for converted in [numerator.div_euclid(4096), numerator / 4096] {
                    let value = i64::from(stored) + converted;
                    assert!(i64::from(range.min) <= value && value <= i64::from(range.max));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn negative_regression_does_not_assume_toward_zero() -> Result<(), String> {
        let range = regress_mmr(2000).ok_or("Missing normalization range")?;
        assert_eq!(
            range,
            ScoreRange {
                min: 1949,
                max: 1951
            }
        );
        let range = regress_mmr(1001).ok_or("Missing normalization range")?;
        assert_eq!(
            range,
            ScoreRange {
                min: 1000,
                max: 1001
            }
        );
        Ok(())
    }

    #[test]
    fn reset_precedes_regression_and_missing_flags_are_not_zeroed() {
        let mut bank = BankReport {
            player_name: None,
            account_id: String::new(),
            path: std::path::PathBuf::new(),
            server_id: None,
            bank_version: 6,
            fields: BTreeMap::from([
                ("lv_alienSelectionHMMR".into(), 2000),
                ("lv_modTKScoreReset".into(), 1),
            ]),
            decode_note: String::new(),
            signature_present: false,
            source_text: String::new(),
        };
        assert_eq!(
            normalized_mmr(OpeningStats::Loaded(&bank), "lv_alienSelectionHMMR", false),
            regress_mmr(100)
        );
        assert_eq!(
            normalized_mmr(OpeningStats::Loaded(&bank), "lv_alienSelectionHMMR", true),
            regress_mmr(2000)
        );
        bank.fields.remove("lv_modTKScoreReset");
        assert_eq!(
            normalized_mmr(OpeningStats::Loaded(&bank), "lv_alienSelectionHMMR", false),
            None
        );
        assert_eq!(
            normalized_mmr(OpeningStats::Unknown("test"), "lv_alienSelectionHMMR", true),
            None
        );
    }
}
