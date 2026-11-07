//! Client orchestration; protocol code only sees an already-connected stream.
use crate::{
    endpoint::{Contact, Endpoint},
    model::Peer,
    platform,
    protocol::{self, Code},
    storage::{Alias, AliasStore, Device, TrustStore},
};
use anyhow::{Context, Result};
use std::future::Future;

pub async fn connect(name: &Alias) -> Result<crate::ssh::Session> {
    let device = Device::open(Device::default_root()?, None)?;
    connect_named(&device, name).await
}
pub async fn connect_named(device: &Device, name: &Alias) -> Result<crate::ssh::Session> {
    let aliases = AliasStore::new(device.root().into());
    let endpoint = aliases.resolve(name)?.with_context(|| {
        format!("name {name} is not registered on this device; run blugit join CODE {name}")
    })?;
    let (session, resolved) = connect_resolved(device, &endpoint).await?;
    if resolved.channel != endpoint.channel {
        aliases.refresh_contact(name, &endpoint, &resolved)?;
    }
    Ok(session)
}
pub async fn connect_as(device: &Device, endpoint: &Endpoint) -> Result<crate::ssh::Session> {
    Ok(connect_resolved(device, endpoint).await?.0)
}
async fn connect_resolved(
    device: &Device,
    endpoint: &Endpoint,
) -> Result<(crate::ssh::Session, Endpoint)> {
    TrustStore::new(device.root().into())
        .host(endpoint)?
        .context("repository not registered; run blugit join first")?;
    crate::resolver::resolve(endpoint, |candidate, probing| async move {
        let stream = if probing {
            platform::probe(&candidate.contact()).await
        } else {
            platform::connect(&candidate.contact()).await
        }
        .context("connect Bluetooth transport")?;
        crate::ssh::connect(stream, device, &candidate)
            .await
            .context("authenticate repository connection")
    })
    .await
}
pub async fn enroll<F, Fut>(
    device: &Device,
    contact: Contact,
    name: &Alias,
    code: F,
) -> Result<Peer>
where
    F: FnOnce(Peer) -> Fut,
    Fut: Future<Output = Result<Code>>,
{
    let reservation = AliasStore::new(device.root().into()).reserve(name)?;
    if let Some(previous) = &reservation.previous {
        anyhow::ensure!(
            previous.address == contact.address,
            "name {name} is already bound to another host; choose a different name"
        );
    }
    let stream = platform::connect(&contact)
        .await
        .context("connect Bluetooth transport")?;
    let (endpoint, host) = protocol::enroll_at(
        stream,
        device,
        contact,
        reservation.previous.as_ref().map(|e| e.share),
        code,
    )
    .await?;
    reservation
        .save(&endpoint)
        .context("device registered, but saving the local name failed; run join again")?;
    Ok(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{endpoint::Channel, model::ShareId};
    #[tokio::test]
    async fn unknown_repository_is_rejected_before_accessing_a_radio() -> Result<()> {
        let d = tempfile::tempdir()?;
        let device = Device::open(d.path().into(), None)?;
        let endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            share: ShareId::generate(),
            channel: Channel::try_from(7)?,
        };
        let error = connect_as(&device, &endpoint)
            .await
            .err()
            .context("unregistered share was accepted")?;
        assert!(error.to_string().contains("not registered"));
        let error = connect_named(&device, &"missing".parse()?)
            .await
            .err()
            .context("unknown name accepted")?;
        assert!(error.to_string().contains("not registered on this device"));
        Ok(())
    }
    #[tokio::test]
    async fn existing_name_cannot_be_enrolled_on_a_different_host() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let device = Device::open(dir.path().into(), None)?;
        let name = "demo".parse()?;
        let endpoint = Endpoint {
            address: "AA:BB:CC:DD:EE:FF".parse()?,
            share: ShareId::generate(),
            channel: Channel::try_from(7)?,
        };
        let store = AliasStore::new(device.root().into());
        store.reserve(&name)?.save(&endpoint)?;
        let contact = Contact {
            address: "11:22:33:44:55:66".parse()?,
            channel: Channel::try_from(7)?,
        };
        let error = enroll(&device, contact, &name, |_| async {
            anyhow::bail!("must fail before radio/code input")
        })
        .await
        .expect_err("host changed");
        assert!(error.to_string().contains("already bound to another host"));
        assert_eq!(store.resolve(&name)?.unwrap().address, endpoint.address);
        // Failed enrollment released the registration lock.
        store.reserve(&name)?;
        Ok(())
    }
}
