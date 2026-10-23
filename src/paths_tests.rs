use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::find_default_root;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "foa-default-paths-{}-{timestamp}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }

    fn directory(&self, relative: &str) -> io::Result<PathBuf> {
        let path = self.0.join(relative);
        fs::create_dir_all(&path)?;
        Ok(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn documents_wins_over_empty_appdata_accounts() -> io::Result<()> {
    let temporary = TestDirectory::new()?;
    let documents = temporary.directory("Documents")?;
    let accounts = temporary.directory("Documents/StarCraft II/Accounts")?;
    let appdata = temporary.directory("AppData/Roaming")?;
    temporary.directory("AppData/Roaming/Blizzard/StarCraft II/Accounts")?;

    assert_eq!(
        find_default_root(Some(documents), None, [], Some(appdata)),
        Some(accounts)
    );
    Ok(())
}

#[test]
fn documents_default_finds_a_readable_bank() -> Result<(), Box<dyn Error>> {
    let temporary = TestDirectory::new()?;
    let documents = temporary.directory("Documents")?;
    let banks = temporary
        .directory("Documents/StarCraft II/Accounts/123456/5-S2-1-987654/Banks/1-S2-1-111111")?;
    let bank = banks.join("RR.SC2Bank");
    fs::write(
        &bank,
        r#"<?xml version="1.0" encoding="utf-8"?>
<Bank version="1">
    <Section name="FA">
        <Key name="Name"><Value string="AutoScan Fixture"/></Key>
    </Section>
</Bank>"#,
    )?;

    let root = find_default_root(Some(documents), None, [], None)
        .ok_or("Documents default was not discovered")?;
    let result = crate::scan::run(&root, "RR.SC2Bank")?;
    assert!(result.issues.is_empty(), "{:?}", result.issues);
    assert_eq!(result.accounts.len(), 1);
    assert_eq!(result.reports.len(), 1);
    let report = result.reports.first().ok_or("Bank report was not found")?;
    assert_eq!(report.path, bank);
    assert_eq!(report.account_id, "123456");
    assert_eq!(report.server_id, Some(5));
    assert_eq!(report.player_name.as_deref(), Some("AutoScan Fixture"));
    Ok(())
}

#[test]
fn redirected_documents_wins_over_home_documents() -> io::Result<()> {
    let temporary = TestDirectory::new()?;
    let documents = temporary.directory("OneDrive/Redirected Documents")?;
    let accounts = temporary.directory("OneDrive/Redirected Documents/StarCraft II/Accounts")?;
    let home = temporary.directory("User")?;
    temporary.directory("User/Documents/StarCraft II/Accounts")?;

    assert_eq!(
        find_default_root(Some(documents), Some(home), [], None),
        Some(accounts)
    );
    Ok(())
}

#[test]
fn home_documents_precedes_onedrive_and_appdata_fallbacks() -> io::Result<()> {
    let temporary = TestDirectory::new()?;
    let home = temporary.directory("User")?;
    let accounts = temporary.directory("User/Documents/StarCraft II/Accounts")?;
    let onedrive = temporary.directory("OneDrive")?;
    temporary.directory("OneDrive/Documents/StarCraft II/Accounts")?;
    let appdata = temporary.directory("AppData/Roaming")?;
    temporary.directory("AppData/Roaming/Blizzard/StarCraft II/Accounts")?;

    assert_eq!(
        find_default_root(
            Some(temporary.0.join("Missing Documents")),
            Some(home),
            [onedrive],
            Some(appdata),
        ),
        Some(accounts)
    );
    Ok(())
}

#[test]
fn onedrive_fallback_skips_missing_candidates() -> io::Result<()> {
    let temporary = TestDirectory::new()?;
    let onedrive = temporary.directory("OneDriveConsumer")?;
    let accounts = temporary.directory("OneDriveConsumer/Documents/StarCraft II/Accounts")?;
    let appdata = temporary.directory("AppData/Roaming")?;
    temporary.directory("AppData/Roaming/Blizzard/StarCraft II/Accounts")?;

    assert_eq!(
        find_default_root(
            Some(temporary.0.join("Missing Documents")),
            Some(temporary.0.join("Missing User")),
            [temporary.0.join("Missing OneDrive"), onedrive],
            Some(appdata),
        ),
        Some(accounts)
    );
    Ok(())
}

#[test]
fn appdata_remains_a_fallback_for_each_supported_layout() -> io::Result<()> {
    for publisher in ["Blizzard", "Blizzard Entertainment", ""] {
        let temporary = TestDirectory::new()?;
        let appdata = temporary.directory("AppData/Roaming")?;
        let accounts = appdata
            .join(publisher)
            .join("StarCraft II")
            .join("Accounts");
        fs::create_dir_all(&accounts)?;

        assert_eq!(
            find_default_root(None, None, [], Some(appdata)),
            Some(accounts)
        );
    }
    Ok(())
}

#[test]
fn missing_directories_do_not_produce_a_default_path() -> io::Result<()> {
    let temporary = TestDirectory::new()?;
    assert_eq!(
        find_default_root(
            Some(temporary.0.join("Documents")),
            Some(temporary.0.join("User")),
            [temporary.0.join("OneDrive")],
            Some(temporary.0.join("AppData/Roaming")),
        ),
        None
    );
    assert_eq!(find_default_root(None, None, [], None), None);
    Ok(())
}
