//! Domain values. No filesystem, CLI, radio or process dependencies.
use anyhow::{Result, ensure};
use rand::{Rng, rand_core::UnwrapErr, rngs::SysRng};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ShareId([u8; 16]);

impl ShareId {
    pub fn generate() -> Self {
        let mut bytes = [0; 16];
        loop {
            UnwrapErr(SysRng).fill_bytes(&mut bytes);
            if bytes != [0; 16] {
                break;
            }
        }
        Self(bytes)
    }
}
impl fmt::Display for ShareId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}
impl FromStr for ShareId {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        ensure!(
            value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()),
            "share ID must be 32 hex digits"
        );
        let mut id = [0; 16];
        hex::decode_to_slice(value, &mut id)?;
        ensure!(id != [0; 16], "invalid all-zero share ID");
        Ok(Self(id))
    }
}
impl TryFrom<String> for ShareId {
    type Error = anyhow::Error;
    fn try_from(v: String) -> Result<Self> {
        v.parse()
    }
}
impl From<ShareId> for String {
    fn from(id: ShareId) -> Self {
        id.to_string()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub name: String,
    pub key: [u8; 32],
    pub ssh_key: [u8; 32],
}
impl Peer {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty()
                && self.name.len() <= 128
                && !self.name.chars().any(char::is_control),
            "device name must be 1..128 bytes without control characters"
        );
        ensure!(self.key != [0; 32], "invalid device public key");
        ensure!(self.ssh_key != [0; 32], "invalid SSH public key");
        Ok(())
    }
    pub fn id(&self) -> String {
        hex::encode(self.key)
    }
    pub fn short_id(&self) -> String {
        hex::encode(&self.key[..6])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_roundtrip_and_reject_invalid_input() -> Result<()> {
        let id = ShareId::generate();
        assert_eq!(id.to_string().parse::<ShareId>()?, id);
        assert_eq!(
            serde_json::from_str::<ShareId>(&serde_json::to_string(&id)?)?,
            id
        );
        for bad in [
            "",
            "../repo",
            "00000000000000000000000000000000",
            "0123456789012345678901234567890z",
        ] {
            assert!(bad.parse::<ShareId>().is_err());
        }
        Ok(())
    }
    #[test]
    fn display_names_never_define_identity() {
        let a = Peer {
            name: "PC".into(),
            key: [1; 32],
            ssh_key: [3; 32],
        };
        let b = Peer {
            name: "PC".into(),
            key: [2; 32],
            ssh_key: [3; 32],
        };
        assert_ne!(a.id(), b.id());
        for name in ["", "  ", "PC\nforged", "\u{1b}[31m"] {
            assert!(
                Peer {
                    name: name.into(),
                    key: [1; 32],
                    ssh_key: [3; 32]
                }
                .validate()
                .is_err()
            );
        }
    }
}
