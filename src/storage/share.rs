use super::atomic::{self, FileLock};
use crate::model::{Peer, ShareId};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MARKER: &str = ".blugit-share.json";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: u32,
    id: ShareId,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    schema: u32,
    id: ShareId,
    repository: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    schema: u32,
    peer: Peer,
}

#[derive(Clone)]
pub struct ShareStore {
    root: PathBuf,
    id: ShareId,
    repository: PathBuf,
}
impl ShareStore {
    pub fn register(root: PathBuf, repository: &Path) -> Result<Self> {
        let repository = dunce::canonicalize(repository)?;
        let _catalog = FileLock::acquire(&root.join("catalog.lock"), true)?;
        let marker = atomic::read::<Marker>(&repository.join(MARKER))?;
        let mut id = if let Some(marker) = marker {
            ensure!(marker.schema == 1, "unsupported share marker schema");
            marker.id
        } else {
            ShareId::generate()
        };
        if let Some(previous) =
            atomic::read::<Metadata>(&root.join("shares").join(id.to_string()).join("share.json"))?
        {
            ensure!(
                previous.schema == 1 && previous.id == id,
                "invalid share metadata"
            );
            // A filesystem copy must not inherit access to a still-existing repo.
            // A move (old path gone) keeps the identity and its authorizations.
            if previous.repository != repository && previous.repository.exists() {
                id = ShareId::generate();
            }
        }
        let store = Self {
            root,
            id,
            repository,
        };
        atomic::write(
            &store.directory().join("share.json"),
            &Metadata {
                schema: 1,
                id,
                repository: store.repository.clone(),
            },
        )?;
        atomic::write_in_existing_directory(
            &store.repository.join(MARKER),
            &Marker { schema: 1, id },
        )?;
        Ok(store)
    }
    pub fn load(root: PathBuf, repository: &Path) -> Result<Self> {
        let repository = dunce::canonicalize(repository)?;
        let marker = atomic::read::<Marker>(&repository.join(MARKER))?
            .context("repository is not shared yet; run blugit host first")?;
        ensure!(marker.schema == 1, "unsupported share marker schema");
        let store = Self {
            root,
            id: marker.id,
            repository,
        };
        let metadata = atomic::read::<Metadata>(&store.directory().join("share.json"))?
            .context("share is not registered for this device")?;
        ensure!(
            metadata.schema == 1
                && metadata.id == store.id
                && metadata.repository == store.repository,
            "share metadata does not match repository"
        );
        Ok(store)
    }
    pub fn id(&self) -> ShareId {
        self.id
    }
    pub fn repository(&self) -> &Path {
        &self.repository
    }
    fn directory(&self) -> PathBuf {
        self.root.join("shares").join(self.id.to_string())
    }
    fn devices(&self) -> PathBuf {
        self.directory().join("devices")
    }
    pub fn member(&self, key: &[u8; 32]) -> Result<Option<Peer>> {
        let Some(member) =
            atomic::read::<Member>(&self.devices().join(format!("{}.json", hex::encode(key))))?
        else {
            return Ok(None);
        };
        ensure!(
            member.schema == 1 && member.peer.key == *key,
            "invalid member record"
        );
        member.peer.validate()?;
        Ok(Some(member.peer))
    }
    pub(crate) fn grant(&self, peer: &Peer) -> Result<()> {
        peer.validate()?;
        let _lock = FileLock::acquire(&self.directory().join("members.lock"), true)?;
        atomic::write(
            &self.devices().join(format!("{}.json", peer.id())),
            &Member {
                schema: 1,
                peer: peer.clone(),
            },
        )
    }
    pub fn members(&self) -> Result<Vec<Peer>> {
        let entries = match fs::read_dir(self.devices()) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut peers = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .context("invalid member filename")?;
            ensure!(
                name.len() == 64 && name.bytes().all(|c| c.is_ascii_hexdigit()),
                "invalid member filename"
            );
            let mut key = [0; 32];
            hex::decode_to_slice(name, &mut key)?;
            if let Some(peer) = self.member(&key)? {
                peers.push(peer);
            }
        }
        peers.sort_by_key(Peer::id);
        Ok(peers)
    }
    pub fn revoke(&self, prefix: &str) -> Result<Peer> {
        ensure!(
            (8..=64).contains(&prefix.len()) && prefix.bytes().all(|c| c.is_ascii_hexdigit()),
            "device ID must be 8..64 hex digits"
        );
        let prefix = prefix.to_ascii_lowercase();
        let _lock = FileLock::acquire(&self.directory().join("members.lock"), true)?;
        let matches: Vec<_> = self
            .members()?
            .into_iter()
            .filter(|peer| peer.id().starts_with(&prefix))
            .collect();
        ensure!(!matches.is_empty(), "no registered device matches that ID");
        ensure!(
            matches.len() == 1,
            "device ID prefix is ambiguous; use a longer ID"
        );
        let peer = matches.into_iter().next().unwrap();
        fs::remove_file(self.devices().join(format!("{}.json", peer.id())))?;
        Ok(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Result<(tempfile::TempDir, ShareStore)> {
        let d = tempfile::tempdir()?;
        let p = d.path().join("repo.git");
        fs::create_dir(&p)?;
        let s = ShareStore::register(d.path().join("state"), &p)?;
        Ok((d, s))
    }
    #[test]
    fn permissions_are_scoped_and_revocation_never_deletes_git_data() -> Result<()> {
        let (d, a) = fixture()?;
        let other = d.path().join("other.git");
        fs::create_dir(&other)?;
        let b = ShareStore::register(d.path().join("state"), &other)?;
        let peer = Peer {
            name: "Guest".into(),
            key: [1; 32],
        };
        a.grant(&peer)?;
        assert!(a.member(&peer.key)?.is_some());
        assert!(b.member(&peer.key)?.is_none());
        fs::write(a.repository().join("objects.dat"), b"keep Git data")?;
        a.revoke(&peer.short_id())?;
        assert!(a.members()?.is_empty());
        assert_eq!(
            fs::read(a.repository().join("objects.dat"))?,
            b"keep Git data"
        );
        assert_eq!(
            ShareStore::register(d.path().join("state"), a.repository())?.id(),
            a.id()
        );
        Ok(())
    }
    #[test]
    fn copy_gets_new_identity_and_move_preserves_identity() -> Result<()> {
        let (d, a) = fixture()?;
        let copy = d.path().join("copy.git");
        fs::create_dir(&copy)?;
        fs::copy(a.repository().join(MARKER), copy.join(MARKER))?;
        let b = ShareStore::register(d.path().join("state"), &copy)?;
        assert_ne!(a.id(), b.id());
        let moved = d.path().join("moved.git");
        fs::rename(a.repository(), &moved)?;
        let moved = ShareStore::register(d.path().join("state"), &moved)?;
        assert_eq!(a.id(), moved.id());
        Ok(())
    }
    #[test]
    fn ambiguous_device_prefix_never_revokes_either_device() -> Result<()> {
        let (_d, a) = fixture()?;
        let mut key = [1; 32];
        a.grant(&Peer {
            name: "same".into(),
            key,
        })?;
        key[31] = 2;
        a.grant(&Peer {
            name: "same".into(),
            key,
        })?;
        assert!(a.revoke("01010101").is_err());
        assert_eq!(a.members()?.len(), 2);
        assert!(a.revoke("../devices").is_err());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn registration_preserves_repository_directory_permissions() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir()?;
        let repo = d.path().join("repo.git");
        fs::create_dir(&repo)?;
        fs::set_permissions(&repo, fs::Permissions::from_mode(0o750))?;
        ShareStore::register(d.path().join("state"), &repo)?;
        assert_eq!(fs::metadata(repo)?.permissions().mode() & 0o777, 0o750);
        Ok(())
    }
}
