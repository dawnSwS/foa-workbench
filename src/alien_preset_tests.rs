#[cfg(not(feature = "role-ban-only"))]
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::{apply, availability};
use crate::model::{BankReport, FIELDS};

fn report(version: u32) -> BankReport {
    BankReport {
        player_name: Some("保留名字".into()),
        account_id: "fixture".into(),
        path: PathBuf::from("Accounts/fixture/5-S2-1-1234567/Banks/map/RR.SC2Bank"),
        server_id: Some(5),
        bank_version: version,
        fields: FIELDS
            .iter()
            .filter(|field| field.since <= version)
            .map(|field| (field.key.to_owned(), 0))
            .collect(),
        decode_note: String::new(),
        signature_present: false,
        source_text: "local Bank fixture".into(),
    }
}

#[cfg(not(feature = "role-ban-only"))]
mod full {
    use super::*;

    #[test]
    fn exact_mmrs_entry_thresholds_and_penalty_repairs_are_applied_together() -> Result<(), String>
    {
        let mut report = report(6);
        for key in [
            "lv_modTKScoreReset",
            "lv_modBan",
            "lv_modAlienBan",
            "lv_flagSpawnDepriortized",
            "lv_modLeftLastGame",
        ] {
            report.fields.insert(key.into(), 1);
        }
        report.fields.insert("lv_autoModTrollAlien".into(), 9999);
        report
            .fields
            .insert("lv_autoModTemphostbangames".into(), 20);
        report.fields.insert("player_id".into(), 7654321);
        let original = report.fields.clone();
        let preset = apply(&report, &report.fields)?;

        assert_eq!(preset.fields["lv_alienSelectionHMMR"], 9999);
        assert_eq!(preset.fields["lv_alienSelectionAMMR"], 1650);
        assert_eq!(preset.fields["lv_statsGamesPlayed"] + 1, 10);
        assert_eq!(preset.fields["lv_statsGamesWonHost"], 5);
        assert_eq!(preset.fields["player_id"], 1234567);
        for role in 1..=11 {
            assert_eq!(
                preset.fields[&format!("lv_statsGamesPlayedAsRole[{role}]")] + 1,
                4
            );
        }
        for key in [
            "lv_modTKScoreReset",
            "lv_modBan",
            "lv_modAlienBan",
            "lv_autoModTrollAlien",
            "lv_autoModTemphostbangames",
            "lv_flagSpawnDepriortized",
            "lv_modLeftLastGame",
        ] {
            assert_eq!(preset.fields[key], 0, "{key}");
        }
        assert_eq!(report.fields, original, "the source report is not mutated");
        let actual_changes: BTreeMap<_, _> = preset
            .fields
            .iter()
            .filter(|(key, after)| original.get(*key) != Some(*after))
            .map(|(key, after)| (key.clone(), (original[key], *after)))
            .collect();
        let advertised_changes: BTreeMap<_, _> = preset
            .changes
            .iter()
            .map(|change| {
                assert!(!change.label.is_empty());
                assert!(!change.reason.is_empty());
                (change.key.clone(), (change.before, change.after))
            })
            .collect();
        assert_eq!(actual_changes, advertised_changes);
        assert_eq!(advertised_changes.len(), preset.changes.len());
        Ok(())
    }

    #[test]
    fn thresholds_never_reduce_existing_progress_and_unrelated_edits_survive() -> Result<(), String>
    {
        let report = report(6);
        let mut current = report.fields.clone();
        current.insert("lv_statsGamesPlayed".into(), 800);
        current.insert("lv_statsGamesWonHost".into(), 100);
        current.insert("lv_statsGamesWon".into(), 321);
        current.insert("lv_statsGamesWonHuman".into(), 121);
        current.insert("lv_statsGamesWonSpawn".into(), 200);
        current.insert("lv_modHostBan".into(), 1);
        current.insert("lv_modFake".into(), 1);
        current.insert("lv_modSelfer".into(), 1);
        current.insert("lv_screenShakeDisabled".into(), 1);
        current.insert("lv_statsGamesPlayedAsRole[12]".into(), 37);
        for role in 1..=11 {
            current.insert(format!("lv_statsGamesPlayedAsRole[{role}]"), 10 + role);
            current.insert(format!("lv_modRoleBan[{role}]"), role % 2);
        }
        current.insert("lv_modRoleBan[12]".into(), 1);
        let preset = apply(&report, &current)?;
        let adjustable = [
            "player_id",
            "lv_alienSelectionHMMR",
            "lv_alienSelectionAMMR",
            "lv_modTKScoreReset",
            "lv_modBan",
            "lv_modAlienBan",
            "lv_autoModTrollAlien",
            "lv_autoModTemphostbangames",
            "lv_flagSpawnDepriortized",
            "lv_modLeftLastGame",
        ];
        for (key, before) in &current {
            if !adjustable.contains(&key.as_str()) {
                assert_eq!(preset.fields.get(key), Some(before), "{key}");
            }
        }
        Ok(())
    }

    #[test]
    fn applying_twice_has_no_new_changes() -> Result<(), String> {
        let report = report(6);
        let first = apply(&report, &report.fields)?;
        let second = apply(&report, &first.fields)?;
        assert_eq!(second.fields, first.fields);
        assert!(second.changes.is_empty());
        Ok(())
    }

    #[test]
    fn only_versions_with_the_complete_role_history_can_apply() -> Result<(), String> {
        for version in 4..=6 {
            let report = report(version);
            assert!(availability(&report).is_ok());
            let preset = apply(&report, &report.fields)?;
            assert!(preset.fields.keys().eq(report.fields.keys()));
        }
        for version in [0, 1, 2, 3, 7] {
            let report = report(version);
            assert!(availability(&report).is_err(), "version {version}");
            assert!(apply(&report, &report.fields).is_err());
        }
        Ok(())
    }

    #[test]
    fn read_only_empty_payload_and_unidentified_or_backup_paths_are_rejected() {
        let original = report(6);
        let mut read_only = original.clone();
        read_only.source_text.clear();
        assert!(availability(&read_only).is_err());
        let mut empty = original.clone();
        empty.fields.clear();
        assert!(availability(&empty).is_err());
        for path in [
            "RR.SC2Bank",
            "Accounts/fixture/5-S2-1-1234567/Banks/map/XX.SC2Bank",
            "Accounts/fixture/5-S2-1-1234567/Banks/map/renamed.SC2Bank",
        ] {
            let mut report = original.clone();
            report.path = path.into();
            assert!(availability(&report).is_err(), "{path}");
            assert!(apply(&report, &report.fields).is_err());
        }
    }

    #[test]
    fn hardcoded_bans_and_invalid_player_ids_cannot_be_repaired_by_payload() {
        for handle in [
            "5-S2-1-12355973",
            "5-S2-1-12377679",
            "2-S2-1-11076112",
            "1-S2--1-2203402",
            "1-S2-2-800148",
            "2-S2-2-408510",
            "5-S2-1-0",
            "5-S2-1-100000001",
        ] {
            let mut report = report(6);
            report.path = PathBuf::from("Accounts/fixture")
                .join(handle)
                .join("Banks/map/RR.SC2Bank");
            let before = report.fields.clone();
            assert!(apply(&report, &report.fields).is_err(), "{handle}");
            assert_eq!(report.fields, before);
        }
    }

    #[test]
    fn incomplete_unknown_or_invalid_numeric_fields_fail_without_partial_repairs() {
        let original = report(6);
        let mut omitted = original.fields.clone();
        omitted.remove("lv_statsGamesPlayedAsRole[8]");
        assert!(apply(&original, &omitted).is_err());
        let mut incomplete_source = original.clone();
        incomplete_source.fields = omitted;
        assert!(apply(&incomplete_source, &incomplete_source.fields).is_err());
        let mut unknown_source = original.clone();
        unknown_source.fields.insert("unknown_field".into(), 0);
        assert!(apply(&unknown_source, &unknown_source.fields).is_err());
        let mut invalid = original.fields.clone();
        invalid.insert("lv_alienSelectionHMMR".into(), 10_000);
        let before = invalid.clone();
        assert!(apply(&original, &invalid).is_err());
        assert_eq!(invalid, before);
    }
}

#[cfg(feature = "role-ban-only")]
#[test]
fn ordinary_edition_rejects_the_preset_api_even_with_valid_fields() {
    let report = report(6);
    assert!(availability(&report).is_err());
    assert!(apply(&report, &report.fields).is_err());
}
