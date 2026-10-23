use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{BankReport, FieldKind, FIELDS};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    original: String,
    report: BankReport,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    fn new(version: u32) -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "foa-preset-test-{}-{stamp}-{sequence}",
            std::process::id()
        ));
        let path = root.join("5-S2-1-1234567/Banks/map/RR.SC2Bank");
        fs::create_dir_all(path.parent().ok_or("missing fixture parent")?)
            .map_err(|error| error.to_string())?;
        let mut fields: BTreeMap<String, u64> = FIELDS
            .iter()
            .filter(|field| field.since <= version)
            .map(|field| {
                (
                    field.key.into(),
                    if field.kind == FieldKind::Flag { 1 } else { 0 },
                )
            })
            .collect();
        fields.insert("player_id".into(), 7654321);
        fields.insert("lv_statsGamesWon".into(), 63);
        let payload = super::write::encode(&fields, version, Some("5-S2-1-1234567"))?;
        let original = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Bank version=\"1\"><Section name=\"FA\"><Key name=\"BankVersion\"><Value int=\"{version}\"/></Key><Key name=\"Name\"><Value string=\"保留名字\"/></Key></Section><Section name=\"Encrypted\"><Key name=\"Bank\"><Value string=\"{}\"/></Key></Section><Section name=\"Custom\"><Key name=\"Keep\"><Value string=\"keep-me\"/></Key></Section><Signature value=\"fixture-signature\"/></Bank>\n",
            quick_xml::escape::escape(&payload)
        );
        fs::write(&path, &original).map_err(|error| error.to_string())?;
        let report = super::parse_file(&path, "fixture", Some(5))?;
        Ok(Self {
            root,
            path,
            original,
            report,
        })
    }

    fn sibling_count(&self) -> Result<usize, String> {
        fs::read_dir(self.path.parent().ok_or("missing fixture parent")?)
            .map_err(|error| error.to_string())?
            .try_fold(0, |count, entry| {
                entry.map(|_| count + 1).map_err(|error| error.to_string())
            })
    }
}

#[cfg(not(feature = "role-ban-only"))]
#[test]
fn full_preset_survives_real_bank_encoding_backup_and_disk_readback() -> Result<(), String> {
    for version in 4..=6 {
        let fixture = Fixture::new(version)?;
        let preset = crate::alien_preset::apply(&fixture.report, &fixture.report.fields)?;
        assert!(fixture.report.signature_present);
        let outcome = super::save_modified(&fixture.report, &preset.fields, None, &fixture.path)?;
        let backup = outcome
            .backup_path
            .as_ref()
            .ok_or("missing automatic backup")?;
        assert_eq!(
            fs::read_to_string(backup).map_err(|error| error.to_string())?,
            fixture.original
        );
        let reread = super::parse_file(&fixture.path, "fixture", Some(5))?;
        assert_eq!(reread.fields, preset.fields);
        assert_eq!(reread.bank_version, version);
        assert_eq!(reread.player_name.as_deref(), Some("保留名字"));
        assert_eq!(reread.fields["lv_statsGamesWon"], 63);
        assert_eq!(reread.fields["lv_modRoleBan[8]"], 1);
        assert!(reread.source_text.contains("<Value string=\"keep-me\"/>"));
        assert!(!reread.signature_present);
        assert_eq!(reread.fields["player_id"], 1234567);
        assert_eq!(fixture.sibling_count()?, 2, "only source and backup remain");
    }
    Ok(())
}

#[cfg(not(feature = "role-ban-only"))]
#[test]
fn preset_save_refuses_a_concurrently_changed_source_without_overwriting_it() -> Result<(), String>
{
    let fixture = Fixture::new(6)?;
    let preset = crate::alien_preset::apply(&fixture.report, &fixture.report.fields)?;
    let changed = format!("{}\n<!-- external change -->\n", fixture.original);
    fs::write(&fixture.path, &changed).map_err(|error| error.to_string())?;
    let result = super::save_modified(&fixture.report, &preset.fields, None, &fixture.path);
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(&fixture.path).map_err(|error| error.to_string())?,
        changed
    );
    assert_eq!(
        fixture.sibling_count()?,
        1,
        "no replacement or backup was written"
    );
    Ok(())
}

#[cfg(feature = "role-ban-only")]
#[test]
fn ordinary_save_rejects_direct_mmr_changes_even_if_the_preset_ui_is_bypassed() -> Result<(), String>
{
    let fixture = Fixture::new(6)?;
    let mut forged = fixture.report.fields.clone();
    forged.insert("lv_alienSelectionHMMR".into(), 9999);
    forged.insert("lv_alienSelectionAMMR".into(), 1650);
    assert!(super::save_modified(&fixture.report, &forged, None, &fixture.path).is_err());
    assert_eq!(
        fs::read_to_string(&fixture.path).map_err(|error| error.to_string())?,
        fixture.original
    );
    assert_eq!(fixture.sibling_count()?, 1);
    Ok(())
}
