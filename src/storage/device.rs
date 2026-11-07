use super::atomic;
use crate::model::Peer;
use anyhow::{Context, Result, ensure};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct Record {
    #[zeroize(skip)]
    schema: u32,
    #[zeroize(skip)]
    peer: Peer,
    secret: [u8; 32],
    ssh_secret: [u8; 32],
}
#[derive(Clone)]
pub struct Device {
    root: PathBuf,
    record: Arc<Record>,
}
impl Device {
    pub fn default_root() -> Result<PathBuf> {
        if let Some(root) = std::env::var_os("BLUGIT_HOME") {
            ensure!(!root.is_empty(), "BLUGIT_HOME must not be empty");
            return Ok(root.into());
        }
        Ok(dirs::data_local_dir()
            .context("no user data directory; set BLUGIT_HOME")?
            .join("blugit")
            .join("v2"))
    }
    pub fn open(root: PathBuf, name: Option<String>) -> Result<Self> {
        atomic::private_dir(&root)?;
        let _lock = atomic::FileLock::acquire(&root.join("identity.lock"), true)?;
        let path = root.join("identity.json");
        let stored = atomic::read::<Record>(&path)?;
        let is_new = stored.is_none();
        let mut record = match stored {
            Some(record) => record,
            None => {
                let mut secret = [0; 32];
                OsRng.fill_bytes(&mut secret);
                let key = x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(secret))
                    .to_bytes();
                let mut ssh_secret = [0; 32];
                OsRng.fill_bytes(&mut ssh_secret);
                Record {
                    schema: 2,
                    peer: Peer {
                        name: name.clone().unwrap_or_else(|| {
                            hostname::get()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| "PC".into())
                        }),
                        key,
                        ssh_key: ssh_public(&ssh_secret),
                    },
                    secret,
                    ssh_secret,
                }
            }
        };
        ensure!(record.schema == 2, "unsupported identity schema");
        ensure!(record.secret != [0; 32], "invalid private key");
        ensure!(
            x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(record.secret))
                .to_bytes()
                == record.peer.key,
            "identity public/private keys do not match; restore the identity instead of regenerating it"
        );
        ensure!(record.ssh_secret != [0; 32], "invalid SSH private key");
        ensure!(
            record.peer.ssh_key == ssh_public(&record.ssh_secret),
            "SSH public/private keys do not match; restore the identity instead of regenerating it"
        );
        if let Some(name) = name {
            record.peer.name = name;
            record.peer.validate()?;
            atomic::write(&path, &record)?;
        } else {
            record.peer.validate()?;
            if is_new {
                atomic::write(&path, &record)?;
            }
        }
        Ok(Self {
            root,
            record: Arc::new(record),
        })
    }
    pub fn peer(&self) -> &Peer {
        &self.record.peer
    }
    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.record.secret
    }
    pub(crate) fn ssh_private_key(&self) -> russh::keys::PrivateKey {
        russh::keys::PrivateKey::from(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(
            &self.record.ssh_secret,
        ))
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn ssh_public(seed: &[u8; 32]) -> [u8; 32] {
    russh::keys::ssh_key::private::Ed25519Keypair::from_seed(seed)
        .public
        .0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_survives_rename_and_restart() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let d = Device::open(dir.path().into(), Some("A".into()))?;
        let key = d.peer().key;
        let ssh_key = d.peer().ssh_key;
        assert_eq!(
            Device::open(dir.path().into(), Some("B".into()))?
                .peer()
                .key,
            key
        );
        assert_eq!(Device::open(dir.path().into(), None)?.peer().name, "B");
        assert_eq!(
            Device::open(dir.path().into(), None)?.peer().ssh_key,
            ssh_key
        );
        assert_ne!(key, ssh_key);
        Ok(())
    }
    #[test]
    fn damaged_or_mismatched_keys_fail_closed() -> Result<()> {
        let dir = tempfile::tempdir()?;
        Device::open(dir.path().into(), None)?;
        let path = dir.path().join("identity.json");
        let mut value: serde_json::Value = atomic::read(&path)?.unwrap();
        let original = value.clone();
        value["peer"]["ssh_key"] = serde_json::to_value([1u8; 32])?;
        atomic::write(&path, &value)?;
        assert!(Device::open(dir.path().into(), None).is_err());
        value = original;
        value["peer"]["key"] = serde_json::to_value([1u8; 32])?;
        atomic::write(&path, &value)?;
        assert!(Device::open(dir.path().into(), None).is_err());
        std::fs::write(&path, b"broken")?;
        assert!(Device::open(dir.path().into(), None).is_err());
        assert_eq!(std::fs::read(path)?, b"broken");
        Ok(())
    }
    #[test]
    fn concurrent_initialization_uses_one_key() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = dir.path().to_owned();
                std::thread::spawn(move || Device::open(path, None).unwrap().peer().key)
            })
            .collect();
        let keys: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(keys.iter().all(|key| *key == keys[0]));
        Ok(())
    }
}
