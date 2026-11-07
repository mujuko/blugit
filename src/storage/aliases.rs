//! Local Git names are durable bindings to a repository, not discovery names.
use super::{FileLock, atomic};
use crate::endpoint::Endpoint;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Alias(String);
impl FromStr for Alias {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        ensure!(
            (1..=32).contains(&value.len())
                && value.as_bytes()[0].is_ascii_alphanumeric()
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "name must be 1..32 ASCII letters/digits, hyphens or underscores, starting with a letter/digit"
        );
        Ok(Self(value.to_ascii_lowercase()))
    }
}
impl Alias {
    pub fn from_url(url: &str) -> Result<Self> {
        url.strip_prefix("blugit://")
            .ok_or_else(|| anyhow::anyhow!("expected blugit://NAME"))?
            .parse()
    }
    pub fn url(&self) -> String {
        format!("blugit://{self}")
    }
}
impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl TryFrom<String> for Alias {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self> {
        value.parse()
    }
}
impl From<Alias> for String {
    fn from(value: Alias) -> Self {
        value.0
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    name: Alias,
    endpoint: Endpoint,
}
pub struct AliasStore {
    root: PathBuf,
}
impl AliasStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, name: &Alias) -> PathBuf {
        self.root.join("aliases").join(format!("alias-{name}.json"))
    }
    pub fn resolve(&self, name: &Alias) -> Result<Option<Endpoint>> {
        let Some(record) = atomic::read::<Record>(&self.path(name))? else {
            return Ok(None);
        };
        ensure!(
            record.schema == 1 && record.name == *name,
            "invalid registered name record"
        );
        Ok(Some(record.endpoint))
    }
    pub(crate) fn reserve(&self, name: &Alias) -> Result<Reservation> {
        let lock = FileLock::acquire(
            &self.root.join("aliases").join(format!("alias-{name}.lock")),
            false,
        )?;
        let previous = self.resolve(name)?;
        Ok(Reservation {
            path: self.path(name),
            name: name.clone(),
            previous,
            _lock: lock,
        })
    }
}
pub(crate) struct Reservation {
    path: PathBuf,
    name: Alias,
    pub previous: Option<Endpoint>,
    _lock: FileLock,
}
impl Reservation {
    pub fn save(&self, endpoint: &Endpoint) -> Result<()> {
        if let Some(previous) = &self.previous {
            ensure!(
                previous.address == endpoint.address && previous.share == endpoint.share,
                "name {} is already bound to another repository; choose a different name",
                self.name
            );
        }
        atomic::write(
            &self.path,
            &Record {
                schema: 1,
                name: self.name.clone(),
                endpoint: endpoint.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{endpoint::Channel, model::ShareId};
    #[test]
    fn names_are_safe_and_urls_are_unambiguous() -> Result<()> {
        assert_eq!(
            Alias::from_url("blugit://My_Repo-1")?.url(),
            "blugit://my_repo-1"
        );
        for invalid in [
            "",
            "../x",
            "a/b",
            "a?x",
            "a#x",
            "a:7",
            "a%2fb",
            "-x",
            "a b",
            "日本語",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(invalid.parse::<Alias>().is_err());
        }
        assert!(Alias::from_url("https://demo").is_err());
        Ok(())
    }
    #[test]
    fn reservations_prevent_rebinding_and_concurrent_registration() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = AliasStore::new(dir.path().into());
        let name = "Demo".parse()?;
        let mut endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            channel: Channel::try_from(7)?,
            share: ShareId::generate(),
        };
        let reservation = store.reserve(&name)?;
        assert!(store.reserve(&name).is_err());
        assert!(store.resolve(&name)?.is_none());
        reservation.save(&endpoint)?;
        drop(reservation);
        endpoint.channel = Channel::try_from(9)?;
        store.reserve(&name)?.save(&endpoint)?;
        assert_eq!(
            store.resolve(&"demo".parse()?)?.unwrap().channel,
            endpoint.channel
        );
        endpoint.share = ShareId::generate();
        assert!(store.reserve(&name)?.save(&endpoint).is_err());
        let other = "other".parse()?;
        drop(store.reserve(&other)?);
        assert!(store.resolve(&other)?.is_none());
        // Prefixing filenames makes even Windows device names safe.
        store.reserve(&"con".parse()?)?.save(&endpoint)?;
        Ok(())
    }
    #[test]
    fn corrupt_name_records_fail_instead_of_resolving_another_repository() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = AliasStore::new(dir.path().into());
        let name = "demo".parse()?;
        let endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            channel: Channel::try_from(7)?,
            share: ShareId::generate(),
        };
        atomic::write(
            &store.path(&name),
            &Record {
                schema: 1,
                name: "other".parse()?,
                endpoint,
            },
        )?;
        assert!(store.resolve(&name).is_err());
        assert!(store.reserve(&name).is_err());
        Ok(())
    }
}
