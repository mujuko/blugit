use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 1024 * 1024,
        "state file too large: {}",
        path.display()
    );
    Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
        format!("invalid state file: {}", path.display())
    })?))
}
pub fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("state file has no parent")?;
    private_dir(parent)?;
    write_in_existing_directory(path, value)
}
// The repository marker shares a directory with Git data; do not chmod it.
pub fn write_in_existing_directory(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("state file has no parent")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("replace {}", path.display()))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub struct FileLock(File);
impl FileLock {
    pub fn acquire(path: &Path, wait: bool) -> Result<Self> {
        private_dir(path.parent().context("lock file has no parent")?)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        if wait {
            fs2::FileExt::lock_exclusive(&file)
        } else {
            fs2::FileExt::try_lock_exclusive(&file)
        }
        .with_context(|| format!("another blugit process holds {}", path.display()))?;
        Ok(Self(file))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_replacement_and_process_lock_lifetime() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("value.json");
        write(&path, &vec![1, 2, 3])?;
        write(&path, &vec![4])?;
        assert_eq!(read::<Vec<u8>>(&path)?, Some(vec![4]));
        let lock = FileLock::acquire(&dir.path().join("state.lock"), false)?;
        assert!(FileLock::acquire(&dir.path().join("state.lock"), false).is_err());
        drop(lock);
        assert!(FileLock::acquire(&dir.path().join("state.lock"), false).is_ok());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn secret_files_are_private() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("secret.json");
        write(&path, &42)?;
        assert_eq!(fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(dir.path())?.permissions().mode() & 0o777,
            0o700
        );
        Ok(())
    }
}
