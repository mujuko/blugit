//! Versioned, atomic state. Never deletes Git repository data.
mod aliases;
mod atomic;
mod device;
mod share;
mod trust;

pub use aliases::{Alias, AliasStore};
pub(crate) use atomic::FileLock;
pub use device::Device;
pub use share::ShareStore;
pub use trust::TrustStore;
