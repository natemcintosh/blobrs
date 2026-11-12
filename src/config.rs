//! Saved storage accounts. Names are validated before they reach URLs or config.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccountName(String);

impl AccountName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AccountName {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let name = value.trim();
        if !(3..=24).contains(&name.len())
            || !name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        {
            return Err("Use 3–24 lowercase letters and numbers (account name only).");
        }
        Ok(Self(name.to_owned()))
    }
}

impl From<AccountName> for String {
    fn from(name: AccountName) -> Self {
        name.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    version: u32,
    storage_accounts: BTreeSet<AccountName>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            storage_accounts: BTreeSet::new(),
        }
    }
}

impl Config {
    pub fn path() -> io::Result<PathBuf> {
        dirs::config_dir()
            .map(|dir| dir.join("blobrs/config.json"))
            .ok_or_else(|| io::Error::other("Cannot determine user config directory"))
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let config: Self = serde_json::from_slice(&bytes)?;
        if config.version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported config version",
            ));
        }
        Ok(config)
    }

    pub fn accounts(&self) -> &BTreeSet<AccountName> {
        &self.storage_accounts
    }

    /// Reload under a separate lock and replace atomically so other instances'
    /// additions survive, and a failed write leaves the existing config intact.
    pub fn add_account(path: &Path, account: AccountName) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing config directory"))?;
        fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut config = Self::load(path)?;
        if config.storage_accounts.insert(account) {
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            serde_json::to_writer_pretty(&mut temp, &config)?;
            temp.write_all(b"\n")?;
            temp.as_file().sync_all()?;
            temp.persist(path).map_err(|error| error.error)?;
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_account_names_including_deserialized_names() {
        for name in [
            "",
            "ab",
            "UPPER",
            "has-dash",
            "has space",
            "https://account.blob.core.windows.net",
            "abcdefghijklmnopqrstuvwxy",
            "ééé",
        ] {
            assert!(AccountName::try_from(name.to_owned()).is_err(), "{name}");
            assert!(serde_json::from_str::<AccountName>(&format!("{name:?}")).is_err());
        }
        for name in ["abc", "123", "abcdefghijklmnopqrstuvwx"] {
            assert_eq!(
                AccountName::try_from(format!(" {name} ")).unwrap().as_str(),
                name
            );
        }
    }

    #[test]
    fn accounts_survive_restart_and_duplicates_are_not_added() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.json");
        assert!(Config::load(&path).unwrap().accounts().is_empty());
        for name in ["second", "first", "second"] {
            Config::add_account(&path, AccountName::try_from(name.to_owned()).unwrap()).unwrap();
        }
        let saved = Config::load(&path).unwrap();
        assert_eq!(
            saved
                .accounts()
                .iter()
                .map(AccountName::as_str)
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn invalid_or_future_config_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        for contents in [
            "broken",
            r#"{"version":2,"storage_accounts":[]}"#,
            r#"{"version":1,"storage_accounts":["INVALID"]}"#,
        ] {
            fs::write(&path, contents).unwrap();
            assert!(
                Config::add_account(&path, AccountName::try_from("valid".to_owned()).unwrap())
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn concurrent_instances_preserve_all_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::thread::scope(|scope| {
            for i in 0..16 {
                let path = &path;
                scope.spawn(move || {
                    Config::add_account(path, AccountName::try_from(format!("account{i}")).unwrap())
                        .unwrap()
                });
            }
        });
        assert_eq!(Config::load(&path).unwrap().accounts().len(), 16);
    }
}
