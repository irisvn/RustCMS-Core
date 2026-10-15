//! The S3-compatible driver (feature `s3`): a private bucket through `object_store`.

use super::*;
use futures_util::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt};

/// A private S3-compatible bucket.
pub struct S3Storage {
    store: Arc<dyn ObjectStore>,
    staging: PathBuf,
    control: std::time::Duration,
}

/// Uploads are staged under the application's own storage root (never a
/// shared system directory), in a directory only this user may open.
const STAGING: &str = ".s3-staging";

/// Creates (or checks) the staging directory: a real directory, not a
/// symbolic link, private to this user.
fn private_dir(dir: &Path) -> Result<(), StorageError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(StorageError::Refused(format!("{} is not a directory", dir.display())));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    Ok(())
}

impl S3Storage {
    /// The bucket of `config`, staging uploads under `root`.
    pub fn new(config: &S3Config, root: &Path) -> Result<Self, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Backend("`storage.s3.bucket` is required".into()));
        }
        if !(1..=600).contains(&config.timeout_secs) {
            return Err(StorageError::Backend("`storage.s3.timeout_secs` must be between 1 and 600".into()));
        }
        if !(config.timeout_secs..=86_400).contains(&config.transfer_timeout_secs) {
            return Err(StorageError::Backend(
                "`storage.s3.transfer_timeout_secs` must be between `timeout_secs` and 86400".into(),
            ));
        }
        let control = std::time::Duration::from_secs(config.timeout_secs);
        // A request may move data for `transfer_timeout_secs` (a slow but
        // healthy upload part); control requests get their own deadline below.
        // `with_client_options` replaces every client option: `allow_http` goes here too.
        let client = object_store::ClientOptions::new()
            .with_allow_http(config.allow_http)
            .with_timeout(std::time::Duration::from_secs(config.transfer_timeout_secs))
            .with_connect_timeout(control.min(std::time::Duration::from_secs(10)));
        let retry = object_store::RetryConfig {
            max_retries: 3,
            retry_timeout: control,
            ..object_store::RetryConfig::default()
        };
        let mut builder = AmazonS3Builder::from_env()
            .with_bucket_name(&config.bucket)
            .with_region(config.region.clone().unwrap_or_else(|| "us-east-1".into()))
            .with_client_options(client)
            .with_retry(retry);
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint).with_virtual_hosted_style_request(false);
        }
        let store = builder.build().map_err(|e| StorageError::Backend(e.to_string()))?;
        let staging = root.join(STAGING);
        private_dir(&staging)?;
        Ok(Self { store: Arc::new(store), staging, control })
    }

    /// `future` within the control deadline (retries included).
    async fn bounded<T>(
        &self,
        future: impl std::future::Future<Output = Result<T, StorageError>>,
    ) -> Result<T, StorageError> {
        tokio::time::timeout(self.control, future).await.unwrap_or_else(|_| {
            Err(StorageError::Backend(format!("the bucket did not answer within {:?}", self.control)))
        })
    }
}

fn backend(err: object_store::Error) -> StorageError {
    match err {
        object_store::Error::NotFound { .. } => StorageError::NotFound,
        other => StorageError::Backend(other.to_string()),
    }
}

#[async_trait]
impl Storage for S3Storage {
    fn disk(&self) -> &'static str {
        "s3"
    }

    fn staging_dir(&self) -> PathBuf {
        self.staging.clone()
    }

    async fn put_file(&self, key: &StorageKey, file: &Path, content_type: &str) -> Result<(), StorageError> {
        use object_store::buffered::BufWriter;
        use tokio::io::AsyncWriteExt;
        let path = ObjectPath::from(key.as_str());
        let attributes = object_store::Attributes::from_iter([(
            object_store::Attribute::ContentType,
            object_store::AttributeValue::from(content_type.to_owned()),
        )]);
        let mut writer = BufWriter::new(self.store.clone(), path).with_attributes(attributes);
        let mut source = tokio::fs::File::open(file).await?;
        if let Err(err) = tokio::io::copy(&mut source, &mut writer).await {
            let _ = writer.abort().await;
            return Err(err.into());
        }
        writer.shutdown().await?;
        Ok(())
    }

    async fn open(&self, key: &StorageKey) -> Result<(Body, u64), StorageError> {
        // Until the headers: the control deadline; the body then streams
        // under the transfer timeout.
        let result =
            self.bounded(async { self.store.get(&ObjectPath::from(key.as_str())).await.map_err(backend) }).await?;
        let len = result.meta.size;
        let stream = result.into_stream().map(|chunk| chunk.map_err(std::io::Error::other));
        Ok((Body::from_stream(stream), len))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        self.bounded(async {
            match self.store.delete(&ObjectPath::from(key.as_str())).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(err) => Err(backend(err)),
            }
        })
        .await
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, StorageError> {
        self.bounded(async {
            match self.store.head(&ObjectPath::from(key.as_str())).await {
                Ok(_) => Ok(true),
                Err(object_store::Error::NotFound { .. }) => Ok(false),
                Err(err) => Err(backend(err)),
            }
        })
        .await
    }

    async fn sweep(&self, older_than: std::time::Duration) -> Result<u64, StorageError> {
        let dir = self.staging.clone();
        let removed = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            let cutoff = std::time::SystemTime::now() - older_than;
            let mut removed = 0;
            for entry in std::fs::read_dir(&dir)? {
                // Entries may vanish meanwhile (an upload finishing): skip them.
                let Ok(entry) = entry else { continue };
                // Only staged uploads (`<uuid>.upload`), never links or other files.
                if !entry.file_name().to_string_lossy().ends_with(".upload") {
                    continue;
                }
                let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
                if meta.file_type().is_file()
                    && meta.modified().is_ok_and(|m| m < cutoff)
                    && std::fs::remove_file(entry.path()).is_ok()
                {
                    removed += 1;
                }
            }
            Ok(removed)
        })
        .await
        .map_err(|e| StorageError::Backend(e.to_string()))??;
        Ok(removed)
    }
}
