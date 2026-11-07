use anyhow::{Context, Result, ensure};
use std::{sync::Mutex, time::Duration};
use tokio::{
    sync::{Semaphore, SemaphorePermit},
    time::Instant,
};

pub(super) struct Gate {
    slot: Semaphore,
    last: Mutex<Option<Instant>>,
    pub lifetime: Duration,
    pub cooldown: Duration,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            slot: Semaphore::new(1),
            last: Mutex::new(None),
            lifetime: Duration::from_secs(180),
            cooldown: Duration::from_secs(10),
        }
    }
}
impl Gate {
    pub fn admit(&self) -> Result<SemaphorePermit<'_>> {
        let permit = self
            .slot
            .try_acquire()
            .context("another registration is pending")?;
        let mut last = self
            .last
            .lock()
            .map_err(|_| anyhow::anyhow!("registration gate poisoned"))?;
        ensure!(
            last.is_none_or(|t| t.elapsed() >= self.cooldown),
            "too many registration requests; wait before trying again"
        );
        *last = Some(Instant::now());
        Ok(permit)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn active_request_and_repeated_issuance_are_limited() -> Result<()> {
        let gate = Gate::default();
        let permit = gate.admit()?;
        tokio::time::advance(Duration::from_secs(11)).await;
        assert!(gate.admit().is_err());
        drop(permit);
        let permit = gate.admit()?;
        drop(permit);
        assert!(gate.admit().is_err());
        tokio::time::advance(Duration::from_secs(10)).await;
        assert!(gate.admit().is_ok());
        Ok(())
    }
}
