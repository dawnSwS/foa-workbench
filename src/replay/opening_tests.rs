use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::{opening_stats, OpeningStats};
use crate::alien_selection::{ExclusionReason, ScoreRange};
use crate::model::{BankReport, FIELDS};
use crate::replay::evidence::{
    PlayerEvidence, ReplayEvidence, SignupChoice, UpgradeFact, SOURCE_HASH,
};
use crate::replay::{
    selection, AlienSelectionReview, ReplayBank, ReplayKey, ReplayPlayer, ReplayReport,
};

fn player(user_id: u32) -> ReplayPlayer {
    ReplayPlayer {
        slot_id: Some(user_id),
        user_id: Some(user_id),
        name: format!("Player {}", user_id + 1),
        toon_handle: format!("5-S2-1-{}", 9_000_000 + user_id),
        banks: Vec::new(),
        selected_bank: None,
        warnings: Vec::new(),
    }
}

fn options(user_id: u32) -> Value {
    json!({
        "_event": "NNet.Game.SUserOptionsEvent",
        "_userid": { "m_userId": user_id },
        "_gameloop": 0,
        "m_gameFullyDownloaded": true,
        "m_isMapToMapTransition": false,
        "m_baseBuildNum": 98_370,
        "m_developmentCheatsEnabled": false,
        "m_testCheatsEnabled": false,
        "m_multiplayerCheatsEnabled": false
    })
}

fn finished() -> Value {
    json!({
        "_event": "NNet.Game.SUserFinishedLoadingSyncEvent",
        "_userid": { "m_userId": 16 },
        "_gameloop": 0
    })
}

fn replay() -> ReplayReport {
    ReplayReport {
        alien_selection: AlienSelectionReview::default(),
        path: PathBuf::new(),
        title: String::new(),
        base_build: 98_370,
        protocol_build: 98_310,
        elapsed_game_loops: 100,
        bank_event_count: 0,
        players: Vec::new(),
        warnings: Vec::new(),
        unmapped_banks: Vec::new(),
        bank_events: Vec::new(),
        loading_sync_events: vec![options(0), finished()],
    }
}

fn bank(name: &str) -> ReplayBank {
    ReplayBank {
        ambiguous_core: false,
        name: name.into(),
        gameloop: 0,
        end_gameloop: Some(0),
        toon_handle: "5-S2-1-9000000".into(),
        signature: Vec::new(),
        sections: BTreeMap::new(),
        report: None,
        warnings: Vec::new(),
    }
}

fn key(data: &str) -> ReplayKey {
    ReplayKey {
        value_type: 3,
        data: data.into(),
        data_is_utf8: true,
        values: Vec::new(),
    }
}

fn loaded(name: &str, user_id: u32) -> ReplayBank {
    let mut result = bank(name);
    result.toon_handle = format!("5-S2-1-{}", 9_000_000 + user_id);
    result.sections.insert(
        "Encrypted".into(),
        BTreeMap::from([("Bank".into(), key("decoded-test-payload"))]),
    );
    let mut fields: BTreeMap<String, u64> =
        FIELDS.iter().map(|field| (field.key.into(), 0)).collect();
    fields.insert("player_id".into(), u64::from(9_000_000 + user_id));
    fields.insert("lv_statsGamesPlayed".into(), 20);
    fields.insert("lv_statsGamesPlayedAsRole[11]".into(), 10);
    fields.insert("lv_alienSelectionHMMR".into(), 1000);
    fields.insert("lv_alienSelectionAMMR".into(), 1000);
    result.report = Some(BankReport {
        player_name: None,
        account_id: String::new(),
        path: PathBuf::new(),
        server_id: Some(5),
        bank_version: 6,
        fields,
        decode_note: String::new(),
        signature_present: false,
        source_text: String::new(),
    });
    result
}

#[test]
fn empty_preload_after_complete_sync_uses_map_defaults_not_history() {
    let replay = replay();
    let player = player(0);
    let state = opening_stats(&replay, &player);
    assert!(matches!(state, OpeningStats::Defaults));
    assert_eq!(state.metric("lv_alienSelectionHMMR"), Some(0));
    assert_eq!(state.metric("lv_modBan"), Some(0));
    assert_eq!(state.metric("player_id"), None);
    assert_eq!(state.metric("not_a_map_field"), None);
    assert!(player.banks.is_empty());
}

#[test]
fn missing_or_wrong_loading_evidence_does_not_prove_empty_preload() {
    let player = player(0);
    for evidence in [
        Vec::new(),
        vec![options(0)],
        vec![finished()],
        vec![options(1), finished()],
        vec![options(0), options(0), finished()],
    ] {
        let mut replay = replay();
        replay.loading_sync_events = evidence;
        assert!(matches!(
            opening_stats(&replay, &player),
            OpeningStats::Unknown(_)
        ));
    }
    let mut replay = replay();
    let mut transition = options(0);
    transition["m_isMapToMapTransition"] = Value::Bool(true);
    replay.loading_sync_events = vec![transition, finished()];
    assert!(matches!(
        opening_stats(&replay, &player),
        OpeningStats::Unknown(_)
    ));
}

#[test]
fn completion_before_player_options_does_not_close_the_loading_phase() {
    let mut replay = replay();
    replay.loading_sync_events = vec![finished(), options(0)];
    assert!(matches!(
        opening_stats(&replay, &player(0)),
        OpeningStats::Unknown(_)
    ));
}

#[test]
fn unmapped_opening_bank_prevents_absence_inference() {
    let mut replay = replay();
    replay.unmapped_banks.push(bank("RR"));
    assert!(matches!(
        opening_stats(&replay, &player(0)),
        OpeningStats::Unknown(_)
    ));
}

#[test]
fn corrupt_incomplete_or_ambiguous_payloads_never_become_defaults() {
    let replay = replay();
    for case in 0..4 {
        let mut player = player(0);
        let mut snapshot = loaded("RR", 0);
        match case {
            0 => snapshot.report = None,
            1 => snapshot.end_gameloop = None,
            2 => snapshot.ambiguous_core = true,
            _ => snapshot.end_gameloop = Some(3),
        }
        player.banks.push(snapshot);
        assert!(matches!(
            opening_stats(&replay, &player),
            OpeningStats::Unknown(_)
        ));
    }
}

#[test]
fn late_bank_without_loading_proof_remains_unknown() {
    let mut replay = replay();
    replay.loading_sync_events = vec![options(0)];
    for mut snapshot in [loaded("RR", 0), bank("RR")] {
        let mut player = player(0);
        snapshot.gameloop = 181;
        snapshot.end_gameloop = Some(181);
        player.banks.push(snapshot);
        assert!(matches!(
            opening_stats(&replay, &player),
            OpeningStats::Unknown(_)
        ));
    }
}

#[test]
fn completed_empty_preload_survives_later_checks_and_payload_writes() {
    let replay = replay();
    for later_payload in [false, true] {
        let mut player = player(0);
        // Real regression: the map's five-second anti-read-only check reloads
        // RR twice with just X/X, after selection has already finished.
        for (gameloop, flag) in [(181, "1"), (184, "0")] {
            let mut snapshot = bank("RR");
            snapshot.gameloop = gameloop;
            snapshot.end_gameloop = Some(gameloop);
            let mut value = key(flag);
            value.value_type = 1;
            snapshot
                .sections
                .insert("X".into(), BTreeMap::from([("X".into(), value)]));
            player.banks.push(snapshot);
        }
        if later_payload {
            let mut snapshot = loaded("RR", 0);
            snapshot.gameloop = 200;
            snapshot.end_gameloop = Some(200);
            player.banks.push(snapshot);
        }
        let state = opening_stats(&replay, &player);
        assert!(matches!(state, OpeningStats::Defaults));
        assert_eq!(state.metric("lv_statsGamesPlayed"), Some(0));
        assert_eq!(state.metric("lv_alienSelectionHMMR"), Some(0));
        assert_eq!(state.metric("player_id"), None);
    }
}

#[test]
fn late_primary_or_backup_does_not_change_the_opening_choice() -> Result<(), String> {
    let replay = replay();
    for case in 0..3 {
        let mut player = player(0);
        let (opening, late_name) = match case {
            0 => (bank("RR"), "XX"),
            1 => (loaded("RR", 0), "XX"),
            _ => (loaded("XX", 0), "RR"),
        };
        let mut late = loaded(late_name, 0);
        late.gameloop = 181;
        late.end_gameloop = Some(181);
        late.report
            .as_mut()
            .ok_or("missing late fixture report")?
            .fields
            .insert("lv_alienSelectionHMMR".into(), 2000);
        player.banks = vec![opening, late];
        let state = opening_stats(&replay, &player);
        if case == 0 {
            assert!(matches!(state, OpeningStats::Defaults));
            assert_eq!(state.metric("lv_alienSelectionHMMR"), Some(0));
        } else {
            assert!(matches!(state, OpeningStats::Loaded(_)));
            assert_eq!(state.metric("lv_alienSelectionHMMR"), Some(1000));
        }
    }
    Ok(())
}

#[test]
fn encrypted_section_without_key_selects_primary_defaults_over_backup() {
    let mut replay = replay();
    replay.loading_sync_events.clear();
    let mut player = player(0);
    let mut primary = bank("RR");
    primary.sections.insert("Encrypted".into(), BTreeMap::new());
    player.banks = vec![primary, loaded("XX", 0)];
    assert!(matches!(
        opening_stats(&replay, &player),
        OpeningStats::Defaults
    ));
}

#[test]
fn primary_without_encrypted_section_selects_complete_backup() {
    let mut replay = replay();
    replay.loading_sync_events.clear();
    let mut player = player(0);
    player.banks = vec![bank("RR"), loaded("XX", 0)];
    let state = opening_stats(&replay, &player);
    assert!(matches!(state, OpeningStats::Loaded(_)));
    assert_eq!(state.metric("lv_alienSelectionHMMR"), Some(1000));
}

#[test]
fn missing_backup_without_loading_proof_keeps_empty_primary_unknown() {
    let mut replay = replay();
    replay.loading_sync_events.clear();
    let mut player = player(0);
    player.banks.push(bank("RR"));
    assert!(matches!(
        opening_stats(&replay, &player),
        OpeningStats::Unknown(_)
    ));
}

#[test]
fn old_versions_use_only_fields_the_map_leaves_unassigned() -> Result<(), String> {
    let mut snapshot = loaded("RR", 0);
    let report = snapshot.report.as_mut().ok_or("missing fixture report")?;
    report.fields.clear();
    report.bank_version = 0;
    assert_eq!(OpeningStats::Loaded(report).metric("lv_modBan"), Some(0));
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_alienSelectionHMMR"),
        Some(0)
    );
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_statsGamesPlayed"),
        None
    );
    report.bank_version = 2;
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_modTKScoreReset"),
        Some(0)
    );
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_flagSpawnDepriortized"),
        Some(0)
    );
    assert_eq!(OpeningStats::Loaded(report).metric("lv_modBan"), None);
    report.bank_version = 3;
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_statsGamesPlayedAsRole[11]"),
        Some(0)
    );
    assert_eq!(
        OpeningStats::Loaded(report).metric("lv_modTKScoreReset"),
        None
    );
    report.bank_version = 6;
    for field in [
        "lv_modBan",
        "lv_alienSelectionHMMR",
        "lv_modTKScoreReset",
        "lv_statsGamesPlayedAsRole[11]",
    ] {
        assert_eq!(OpeningStats::Loaded(report).metric(field), None);
    }
    Ok(())
}

#[test]
fn mismatched_identity_and_conflicting_cores_remain_unknown() -> Result<(), String> {
    let replay = replay();
    let mut player = player(0);
    player.banks.push(loaded("RR", 1));
    assert!(matches!(
        opening_stats(&replay, &player),
        OpeningStats::Unknown(_)
    ));
    let first = loaded("RR", 0);
    let mut second = loaded("RR", 0);
    let section = second
        .sections
        .get_mut("Encrypted")
        .ok_or("missing fixture section")?;
    section.insert("Bank".into(), key("conflicting-payload"));
    player.banks = vec![first, second];
    assert!(matches!(
        opening_stats(&replay, &player),
        OpeningStats::Unknown(_)
    ));
    Ok(())
}

#[test]
fn proven_default_player_contributes_100_to_room_mean() -> Result<(), String> {
    let mut replay = replay();
    replay.loading_sync_events = (0..4).map(options).chain([finished()]).collect();
    replay.players = (0..4)
        .map(|user| {
            let mut player = player(user);
            if user < 3 {
                player.banks.push(loaded("RR", user));
            }
            player
        })
        .collect();
    let evidence = ReplayEvidence {
        source_hash: SOURCE_HASH,
        map_namespace: 178_950,
        pve: Some(false),
        players: (0..4)
            .map(|user| PlayerEvidence {
                player_id: user + 1,
                user_id: user,
                slot_id: user,
                working_set_slot_id: user,
                toon_handle: format!("5-S2-1-{}", 9_000_000 + user),
                signup: Some(SignupChoice::Both),
                signup_raw: Some("0004".into()),
                role: Some(11),
                role_event: None,
            })
            .collect(),
        opening_host: Some(UpgradeFact {
            gameloop: 4,
            player_id: 1,
            upgrade: "AlienHost".into(),
            delta: 1,
        }),
        opening_successor: Some(UpgradeFact {
            gameloop: 4,
            player_id: 2,
            upgrade: "OriginalSuccessor".into(),
            delta: 1,
        }),
        warnings: Vec::new(),
    };
    let review = selection::review(&replay, Ok(evidence));
    let analysis = review.analysis.ok_or("missing selection analysis")?;
    assert_eq!(analysis.average_hmmr, Some(ScoreRange::point(775)));
    for round in &analysis.rounds {
        assert!(round.target_mmr.is_some());
        assert!(round
            .players
            .iter()
            .filter(|player| player.eligible == Some(true))
            .all(|player| player.breakdown.total_range.is_some()));
        let newcomer = round
            .players
            .iter()
            .find(|player| player.id == 4)
            .ok_or("missing newcomer")?;
        assert!(newcomer
            .exclusions
            .iter()
            .any(|exclusion| exclusion.reason == ExclusionReason::NewPlayer));
    }
    Ok(())
}

#[cfg(not(feature = "role-ban-only"))]
#[test]
fn alien_preset_repairs_all_assigned_roles_without_promising_a_win() -> Result<(), String> {
    use crate::alien_selection::ranges::{compare, Verdict};
    use crate::alien_selection::{Availability, MmrSource};

    for role in 1..=11 {
        let mut replay = replay();
        replay.loading_sync_events = (0..4).map(options).chain([finished()]).collect();
        for user in 0..4 {
            let mut player = player(user);
            let mut snapshot = loaded("RR", user);
            if user < 2 {
                let report = snapshot.report.as_mut().ok_or("missing fixture report")?;
                report.path = PathBuf::from(format!(
                    "/synthetic/Accounts/123/5-S2-1-{}/Banks/1/RR.SC2Bank",
                    9_000_000 + user
                ));
                // The preset operates on an editable local-file draft. Replay
                // collection later supplies the resulting decoded fields.
                report.source_text = "synthetic local Bank fixture".into();
                report.fields.insert("player_id".into(), 1);
                report.fields.insert("lv_statsGamesPlayed".into(), 0);
                report.fields.insert("lv_statsGamesWonHost".into(), 0);
                for assigned in 1..=11 {
                    report
                        .fields
                        .insert(format!("lv_statsGamesPlayedAsRole[{assigned}]"), 0);
                }
                for flag in [
                    "lv_modBan",
                    "lv_modAlienBan",
                    "lv_modTKScoreReset",
                    "lv_flagSpawnDepriortized",
                    "lv_modLeftLastGame",
                ] {
                    report.fields.insert(flag.into(), 1);
                }
                report.fields.insert("lv_autoModTrollAlien".into(), 200);
                report
                    .fields
                    .insert("lv_autoModTemphostbangames".into(), 10);
                report.fields = crate::alien_preset::apply(report, &report.fields)?.fields;
            }
            player.banks.push(snapshot);
            replay.players.push(player);
        }
        let evidence = ReplayEvidence {
            source_hash: SOURCE_HASH,
            map_namespace: 178_950,
            pve: Some(false),
            players: (0..4)
                .map(|user| PlayerEvidence {
                    player_id: user + 1,
                    user_id: user,
                    slot_id: user,
                    working_set_slot_id: user,
                    toon_handle: format!("5-S2-1-{}", 9_000_000 + user),
                    signup: Some(SignupChoice::Both),
                    signup_raw: Some("0004".into()),
                    role: Some(if user == 0 { role } else { 11 }),
                    role_event: None,
                })
                .collect(),
            opening_host: None,
            opening_successor: None,
            warnings: Vec::new(),
        };
        let review = selection::review(&replay, Ok(evidence));
        let analysis = review.analysis.ok_or("missing selection analysis")?;
        // Two raw 9999 HMMRs normalize to [9548,9551]; the other two
        // participants retain 1000. All four contribute to the room mean.
        assert_eq!(
            analysis.average_hmmr,
            Some(ScoreRange {
                min: 5274,
                max: 5275
            }),
            "role {role}"
        );
        let first = analysis.rounds.first().ok_or("missing opening round")?;
        assert_eq!(first.availability, Availability::CompleteRanges);
        for id in [1, 2] {
            let candidate = first
                .players
                .iter()
                .find(|candidate| candidate.id == id)
                .ok_or("missing preset candidate")?;
            assert_eq!(candidate.eligible, Some(true), "role {role}, player {id}");
            assert!(candidate.exclusions.is_empty(), "role {role}, player {id}");
            assert_eq!(candidate.mmr_source, Some(MmrSource::Alien));
            assert_eq!(candidate.host_wins, Some(5));
            assert_eq!(
                candidate.used_mmr,
                Some(ScoreRange {
                    min: 1617,
                    max: 1618
                })
            );
            assert_eq!(candidate.breakdown.new_player_penalty, Some(0));
            assert_eq!(candidate.breakdown.new_role_penalty, Some(0));
            assert_eq!(candidate.breakdown.host_ban_penalty, Some(0));
            assert_eq!(candidate.breakdown.deprioritized_penalty, Some(0));
            assert_eq!(candidate.breakdown.left_penalty, Some(0));
            // Two equal presets and equal lobby choices still compete on the
            // bounded added term; neither configuration guarantees selection.
            assert_eq!(compare(first, id, None).verdict, Verdict::Conditional);
        }
    }
    Ok(())
}
