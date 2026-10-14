//! The `local` driver: files under a root directory, opened with `cap-std` so
//! no path can leave it.

use super::*;

/// Files under a root directory. Every operation is anchored to the opened
/// root (`cap-std`): no path, symlink or concurrent rename can lead outside it.
pub struct LocalStorage {
    root: PathBuf,
    dir: Arc<cap_std::fs::Dir>,
}

impl LocalStorage {
    /// Uses (and creates) `root`; fails unless it is a writable directory.
    pub fn new(root: &Path) -> Result<Self, StorageError> {
        std::fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        let dir = cap_std::fs::Dir::open_ambient_dir(&root, cap_std::ambient_authority())?;
        if dir.symlink_metadata(STAGING).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(StorageError::Refused(format!("{STAGING} is a symlink")));
        }
        dir.create_dir_all(STAGING)?;
        // Writable now, not at the first upload.
        let probe = format!("{STAGING}/.probe-{}", uuid::Uuid::now_v7());
        dir.write(&probe, b"ok")?;
        dir.remove_file(&probe)?;
        Ok(Self { root, dir: Arc::new(dir) })
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&cap_std::fs::Dir) -> std::io::Result<T> + Send + 'static,
    ) -> Result<T, StorageError> {
        let dir = self.dir.clone();
        tokio::task::spawn_blocking(move || work(&dir))
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?
            .map_err(StorageError::from)
    }
}

#[async_trait]
impl Storage for LocalStorage {
    fn disk(&self) -> &'static str {
        "local"
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(STAGING)
    }

    async fn put_file(&self, key: &StorageKey, file: &Path, _content_type: &str) -> Result<(), StorageError> {
        let key = key.as_str().to_owned();
        let source = file.to_owned();
        self.blocking(move |dir| {
            let parent = Path::new(&key).parent().unwrap_or(Path::new(""));
            dir.create_dir_all(parent)?;
            // Written next to the target, synced, then renamed: readers never
            // see a partial file, and a committed row never names a lost one.
            let partial = parent.join(format!(".{}.partial", uuid::Uuid::now_v7()));
            let result = (|| {
                let mut out = dir.create(&partial)?;
                std::io::copy(&mut std::fs::File::open(&source)?, &mut out)?;
                out.sync_all()?;
                dir.rename(&partial, dir, &key)?;
                dir.open_dir(parent)?.into_std_file().sync_all()
            })();
            if result.is_err() {
                let _ = dir.remove_file(&partial);
            }
            result
        })
        .await
    }

    async fn open(&self, key: &StorageKey) -> Result<(Body, u64), StorageError> {
        let key = key.as_str().to_owned();
        let file = self.blocking(move |dir| dir.open(&key)).await?;
        let file = tokio::fs::File::from_std(file.into_std());
        let len = file.metadata().await?.len();
        Ok((Body::from_stream(tokio_util::io::ReaderStream::new(file)), len))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let key = key.as_str().to_owned();
        self.blocking(move |dir| match dir.remove_file(&key) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        })
        .await
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, StorageError> {
        let key = key.as_str().to_owned();
        self.blocking(move |dir| Ok(dir.exists(&key))).await
    }

    async fn sweep(&self, older_than: std::time::Duration) -> Result<u64, StorageError> {
        self.blocking(move |dir| {
            let cutoff = std::time::SystemTime::now() - older_than;
            let old = |meta: &cap_std::fs::Metadata| {
                meta.modified().map(|m| m.into_std() < cutoff).unwrap_or(false) && meta.is_file()
            };
            let mut removed = 0;
            // Staged uploads, then partial files next to objects (`yyyy/mm/`).
            for entry in dir.read_dir(STAGING)? {
                let entry = entry?;
                if old(&entry.metadata()?) && dir.remove_file(Path::new(STAGING).join(entry.file_name())).is_ok() {
                    removed += 1;
                }
            }
            for year in dir.entries()? {
                let year = year?;
                let name = year.file_name();
                if name.to_string_lossy().starts_with('.') || !year.file_type()?.is_dir() {
                    continue;
                }
                for month in dir.read_dir(&name)? {
                    let month = month?;
                    if !month.file_type()?.is_dir() {
                        continue;
                    }
                    let path = Path::new(&name).join(month.file_name());
                    for file in dir.read_dir(&path)? {
                        let file = file?;
                        let partial = file.file_name().to_string_lossy().ends_with(".partial");
                        if partial && old(&file.metadata()?) && dir.remove_file(path.join(file.file_name())).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
            Ok(removed)
        })
        .await
    }
}

pub(super) const STAGING: &str = ".staging";
