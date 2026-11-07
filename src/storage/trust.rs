use super::atomic;
use crate::{endpoint::Endpoint, model::Peer};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    endpoint: Endpoint,
    host: Peer,
}
#[derive(Clone)]
pub struct TrustStore {
    root: PathBuf,
}
impl TrustStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    fn path(&self, e: &Endpoint) -> PathBuf {
        self.root
            .join("hosts")
            .join(format!("{}-{}.json", hex::encode(e.address.0), e.share))
    }
    pub fn host(&self, e: &Endpoint) -> Result<Option<Peer>> {
        let Some(record) = atomic::read::<Record>(&self.path(e))? else {
            return Ok(None);
        };
        ensure!(
            record.schema == 1
                && record.endpoint.address == e.address
                && record.endpoint.share == e.share,
            "invalid host trust record"
        );
        record.host.validate()?;
        Ok(Some(record.host))
    }
    pub(crate) fn pin(&self, e: &Endpoint, host: &Peer) -> Result<()> {
        host.validate()?;
        atomic::write(
            &self.path(e),
            &Record {
                schema: 1,
                endpoint: e.clone(),
                host: host.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{endpoint::Channel, model::ShareId};
    #[test]
    fn host_trust_is_scoped_to_address_and_share() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let store = TrustStore::new(dir.path().into());
        let e = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            share: ShareId::generate(),
            channel: Channel::try_from(7)?,
        };
        let peer = Peer {
            name: "Host".into(),
            key: [1; 32],
            ssh_key: [3; 32],
        };
        store.pin(&e, &peer)?;
        assert_eq!(store.host(&e)?, Some(peer));
        let mut different = e;
        different.share = ShareId::generate();
        assert!(store.host(&different)?.is_none());
        Ok(())
    }
}
