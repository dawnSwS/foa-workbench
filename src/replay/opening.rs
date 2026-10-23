//! Selection-time state is distinct from historical Bank evidence.
//! Only the audited map preloads RR and XX for all twelve player slots.
use crate::model::{BankReport, FieldKind, FIELDS};

use super::{unique, ReplayBank, ReplayPlayer, ReplayReport};

#[derive(Clone, Copy)]
pub(super) enum OpeningStats<'a> {
    Loaded(&'a BankReport),
    /// The map never enters its payload-load branch; struct fields start at 0.
    Defaults,
    Unknown(&'static str),
}

impl OpeningStats<'_> {
    pub(super) fn metric(self, key: &str) -> Option<u64> {
        let field = FIELDS.iter().find(|field| field.key == key)?;
        match self {
            Self::Loaded(report) if report.bank_version < field.since => Some(0),
            Self::Loaded(report) => report.metric(key),
            Self::Defaults if field.kind != FieldKind::Identity => Some(0),
            Self::Defaults | Self::Unknown(_) => None,
        }
    }
}

/// Called only after evidence::check_map has matched the audited source.
pub(super) fn opening_stats<'a>(
    replay: &ReplayReport,
    player: &'a ReplayPlayer,
) -> OpeningStats<'a> {
    let loading_complete =
        player.user_id.is_some_and(|user| {
            let options = unique(replay.loading_sync_events.iter().enumerate().filter(
                |(_, event)| {
                    event.get("_event").and_then(serde_json::Value::as_str)
                        == Some("NNet.Game.SUserOptionsEvent")
                        && event
                            .pointer("/_userid/m_userId")
                            .and_then(serde_json::Value::as_u64)
                            == Some(u64::from(user))
                        && event.get("_gameloop").and_then(serde_json::Value::as_u64) == Some(0)
                },
            ));
            // In this replay profile FinishedLoadingSync is emitted by system user
            // 16, not once per human. Requiring a per-human event loses empty banks.
            let Some((options_index, options)) = options else {
                return false;
            };
            let finished = replay
                .loading_sync_events
                .iter()
                .skip(options_index + 1)
                .any(|event| {
                    event.get("_event").and_then(serde_json::Value::as_str)
                        == Some("NNet.Game.SUserFinishedLoadingSyncEvent")
                        && event
                            .pointer("/_userid/m_userId")
                            .and_then(serde_json::Value::as_u64)
                            == Some(16)
                        && event.get("_gameloop").and_then(serde_json::Value::as_u64) == Some(0)
                });
            finished && {
                options
                    .get("m_gameFullyDownloaded")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                    && options
                        .get("m_isMapToMapTransition")
                        .and_then(serde_json::Value::as_bool)
                        == Some(false)
            }
        });
    let unmatched_early = replay.unmapped_banks.iter().any(|bank| bank.gameloop <= 1);
    resolve(player, loading_complete && !unmatched_early)
}

fn resolve(player: &ReplayPlayer, absence_proven: bool) -> OpeningStats<'_> {
    for name in ["RR", "XX"] {
        if !consistent_early_banks(player, name) {
            return OpeningStats::Unknown("开局 RR/XX 同步未结束、跨过载入时点或存在冲突");
        }
    }
    let early = |name: &str| {
        player
            .banks
            .iter()
            .find(|bank| bank.name == name && bank.gameloop <= 1)
    };
    let primary = early("RR");
    let backup = early("XX");
    // Absence is evaluated at the opening, not across the entire replay.
    // The audited map loads stats at loop 1 and selects aliens at loop 4.
    // After Wait(5.0), its anti-read-only check writes X/X and performs two
    // BankReload/BankWait calls (MapScript 26057–26081). Those later snapshots
    // never call gf_LoadBank again and cannot change selection-time inputs.
    // Still require the full stream, normal-load options and loading completion
    // before treating an absent opening file as an empty preload.
    if primary.is_none() && !absence_proven {
        return OpeningStats::Unknown("缺少主存档且未确认完整预载同步，不能推断为空档");
    }
    let selected = if primary.is_some_and(|bank| bank.sections.contains_key("Encrypted")) {
        primary
    } else {
        if backup.is_none() && !absence_proven {
            return OpeningStats::Unknown("主存档无 Encrypted 节，备用存档的预载状态未确定");
        }
        backup
            .filter(|bank| bank.sections.contains_key("Encrypted"))
            .or(primary)
    };
    let Some(selected) = selected else {
        return OpeningStats::Defaults;
    };
    if !selected
        .sections
        .get("Encrypted")
        .is_some_and(|keys| keys.contains_key("Bank"))
    {
        return OpeningStats::Defaults;
    }
    let Some(report) = selected.report.as_ref() else {
        return OpeningStats::Unknown("选中的开局存档包含核心载荷，但载荷无法解码");
    };
    let identity = player
        .toon_handle
        .rsplit('-')
        .next()
        .and_then(|value| value.parse::<u64>().ok());
    if identity.is_none() || report.metric("player_id") != identity {
        return OpeningStats::Unknown("存档玩家 ID 与录像身份不一致，重置路径未完整复原");
    }
    OpeningStats::Loaded(report)
}

fn consistent_early_banks(player: &ReplayPlayer, name: &str) -> bool {
    let mut previous: Option<&ReplayBank> = None;
    for bank in player
        .banks
        .iter()
        .filter(|bank| bank.name == name && bank.gameloop <= 4)
    {
        // A late/incomplete opening transfer cannot prove a missing preload.
        if bank.gameloop > 1
            || !bank.end_gameloop.is_some_and(|end| end <= 1)
            || bank.ambiguous_core
        {
            return false;
        }
        if let Some(previous) = previous {
            if core(previous) != core(bank) {
                return false;
            }
        }
        previous = Some(bank);
    }
    true
}

fn core(bank: &ReplayBank) -> (bool, Option<&super::ReplayKey>, Option<&super::ReplayKey>) {
    (
        bank.sections.contains_key("Encrypted"),
        bank.sections
            .get("Encrypted")
            .and_then(|keys| keys.get("Bank")),
        bank.sections
            .get("FA")
            .and_then(|keys| keys.get("BankVersion")),
    )
}

#[cfg(test)]
#[path = "opening_tests.rs"]
mod tests;
