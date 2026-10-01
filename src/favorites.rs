//! Durable, account-scoped bookmarks. The separate lock survives atomic replacement.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Favorites {
    version: u32,
    accounts: BTreeMap<String, BTreeSet<String>>,
}

impl Default for Favorites {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: BTreeMap::new(),
        }
    }
}

impl Favorites {
    pub fn path() -> io::Result<PathBuf> {
        dirs::data_dir()
            .map(|dir| dir.join("blobrs/favorites.json"))
            .ok_or_else(|| io::Error::other("Cannot determine user data directory"))
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(error),
        };
        let favorites: Self = serde_json::from_slice(&bytes)?;
        if favorites.version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported favorites version",
            ));
        }
        Ok(favorites)
    }

    pub fn contains(&self, account: &str, container: &str) -> bool {
        self.accounts
            .get(account)
            .is_some_and(|names| names.contains(container))
    }

    /// Reload under the lock so concurrent instances preserve one another's edits.
    pub fn toggle(path: &Path, account: &str, container: &str) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing favorites directory"))?;
        fs::create_dir_all(parent)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))?;
        lock.lock()?;
        let mut favorites = Self::load(path)?;
        let names = favorites.accounts.entry(account.to_owned()).or_default();
        if !names.remove(container) {
            names.insert(container.to_owned());
        }
        if names.is_empty() {
            favorites.accounts.remove(account);
        }
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temp, &favorites)?;
        temp.write_all(b"\n")?;
        temp.as_file().sync_all()?;
        temp.persist(path).map_err(|error| error.error)?;
        Ok(favorites)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_persist_and_accounts_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/favorites.json");
        assert!(!Favorites::load(&path).unwrap().contains("one", "reports"));
        Favorites::toggle(&path, "one", "reports").unwrap();
        Favorites::toggle(&path, "two", "reports").unwrap();
        let saved = Favorites::toggle(&path, "one", "reports").unwrap();
        assert!(!saved.contains("one", "reports"));
        assert!(Favorites::load(&path).unwrap().contains("two", "reports"));
    }

    #[test]
    fn invalid_or_future_files_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("favorites.json");
        for contents in ["broken", r#"{"version":2,"accounts":{}}"#] {
            fs::write(&path, contents).unwrap();
            assert!(Favorites::toggle(&path, "account", "reports").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        }
    }

    #[test]
    fn concurrent_writers_preserve_every_favorite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("favorites.json");
        std::thread::scope(|scope| {
            for i in 0..16 {
                let path = &path;
                scope.spawn(move || {
                    Favorites::toggle(path, "account", &format!("container-{i}")).unwrap()
                });
            }
        });
        assert_eq!(
            Favorites::load(&path).unwrap().accounts["account"].len(),
            16
        );
    }
}
