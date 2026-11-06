//! Disposable, file-backed identity storage for the security spike, not a team DB.
use crate::endpoint::Address;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub name: String,
    pub key: [u8; 32],
}

impl Peer {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 128
                && !self.name.chars().any(char::is_control),
            "invalid device name"
        );
        ensure!(self.key != [0; 32], "invalid device key");
        Ok(())
    }

    pub fn short_id(&self) -> String {
        hex::encode(&self.key[..6])
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    peer: Peer,
    private: [u8; 32],
}

#[derive(Clone)]
pub struct State {
    root: PathBuf,
    identity: Identity,
}

impl State {
    pub fn open(root: PathBuf, name: Option<String>) -> Result<Self> {
        fs::create_dir_all(&root).context("create spike state directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let path = root.join("identity.json");
        if !path.exists() {
            let keys = snow::Builder::new("Noise_KK_25519_ChaChaPoly_BLAKE2s".parse()?)
                .generate_keypair()?;
            let identity = Identity {
                peer: Peer {
                    name: name.clone().unwrap_or_else(|| {
                        hostname::get()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| "PC".into())
                    }),
                    key: keys
                        .public
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("invalid generated public key"))?,
                },
                private: keys
                    .private
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("invalid generated private key"))?,
            };
            identity.peer.validate()?;
            // Concurrent first-time helper processes must not replace each other's key.
            let file = stage_json(&root, &identity)?;
            if let Err(error) = file.persist_noclobber(&path) {
                ensure!(
                    error.error.kind() == std::io::ErrorKind::AlreadyExists,
                    "save identity: {}",
                    error.error
                );
            }
        }
        let mut identity: Identity = serde_json::from_slice(&fs::read(&path)?)
            .context("read spike identity (do not silently regenerate a damaged key)")?;
        identity.peer.validate()?;
        ensure!(identity.private != [0; 32], "invalid private key");
        if let Some(name) = name {
            identity.peer.name = name;
            identity.peer.validate()?;
            stage_json(&root, &identity)?.persist(&path)?;
        }
        Ok(Self { root, identity })
    }

    pub fn default_root() -> Result<PathBuf> {
        if let Some(root) = std::env::var_os("BLUGIT_STATE_DIR") {
            ensure!(!root.is_empty(), "BLUGIT_STATE_DIR must not be empty");
            return Ok(root.into());
        }
        Ok(dirs::data_local_dir()
            .context("no user data directory; set BLUGIT_STATE_DIR")?
            .join("blugit")
            .join("prototype-security"))
    }

    pub fn peer(&self) -> &Peer {
        &self.identity.peer
    }
    pub fn private(&self) -> &[u8; 32] {
        &self.identity.private
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn member(&self, key: &[u8; 32]) -> Result<Option<Peer>> {
        self.read_peer(&self.root.join(format!("member-{}.json", hex::encode(key))))
    }

    pub fn save_member(&self, peer: &Peer) -> Result<()> {
        self.save_peer(
            &self
                .root
                .join(format!("member-{}.json", hex::encode(peer.key))),
            peer,
        )
    }

    pub fn host(&self, address: Address) -> Result<Option<Peer>> {
        self.read_peer(&self.host_path(address))
    }

    pub fn save_host(&self, address: Address, peer: &Peer) -> Result<()> {
        self.save_peer(&self.host_path(address), peer)
    }

    fn host_path(&self, address: Address) -> PathBuf {
        self.root
            .join(format!("host-{}.json", hex::encode(address.0)))
    }

    fn read_peer(&self, path: &Path) -> Result<Option<Peer>> {
        match fs::read(path) {
            Ok(bytes) => {
                let peer: Peer = serde_json::from_slice(&bytes)?;
                peer.validate()?;
                Ok(Some(peer))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn save_peer(&self, path: &Path, peer: &Peer) -> Result<()> {
        peer.validate()?;
        stage_json(&self.root, peer)?.persist(path)?;
        Ok(())
    }
}

fn stage_json(root: &Path, value: &impl Serialize) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    serde_json::to_writer(&mut file, value)?;
    file.flush()?;
    file.as_file().sync_all()?;
    Ok(file)
}
