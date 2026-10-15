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
}

/// Uploads are staged here before they are sent (one directory for every
/// S3 disk of the host; `sweep` removes what interrupted uploads left).
fn staging() -> PathBuf {
    std::env::temp_dir().join("rustcms-staging-s3")
}

impl S3Storage {
    pub fn new(config: &S3Config) -> Result<Self, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Backend("`storage.s3.bucket` is required".into()));
        }
        if !(1..=600).contains(&config.timeout_secs) {
            return Err(StorageError::Backend("`storage.s3.timeout_secs` must be between 1 and 600".into()));
        }
        // A slow bucket fails a request within the budget instead of holding
        // a database transaction (prune) or a worker for minutes.
        let budget = std::time::Duration::from_secs(config.timeout_secs);
        // `with_client_options` replaces every client option: `allow_http` goes here too.
        let client = object_store::ClientOptions::new()
            .with_allow_http(config.allow_http)
            .with_timeout(budget)
            .with_connect_timeout(budget.min(std::time::Duration::from_secs(10)));
        let retry =
            object_store::RetryConfig { max_retries: 3, retry_timeout: budget, ..object_store::RetryConfig::default() };
        let mut builder = AmazonS3Builder::from_env()
            .with_bucket_name(&config.bucket)
            .with_region(config.region.clone().unwrap_or_else(|| "us-east-1".into()))
            .with_client_options(client)
            .with_retry(retry);
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint).with_virtual_hosted_style_request(false);
        }
        let store = builder.build().map_err(|e| StorageError::Backend(e.to_string()))?;
        std::fs::create_dir_all(staging())?;
        Ok(Self { store: Arc::new(store), staging: staging() })
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
        let result = self.store.get(&ObjectPath::from(key.as_str())).await.map_err(backend)?;
        let len = result.meta.size;
        let stream = result.into_stream().map(|chunk| chunk.map_err(std::io::Error::other));
        Ok((Body::from_stream(stream), len))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        match self.store.delete(&ObjectPath::from(key.as_str())).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(err) => Err(backend(err)),
        }
    }

    async fn exists(&self, key: &StorageKey) -> Result<bool, StorageError> {
        match self.store.head(&ObjectPath::from(key.as_str())).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => Ok(false),
            Err(err) => Err(backend(err)),
        }
    }

    async fn sweep(&self, older_than: std::time::Duration) -> Result<u64, StorageError> {
        let dir = self.staging.clone();
        let removed = tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            let cutoff = std::time::SystemTime::now() - older_than;
            let mut removed = 0;
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                if meta.is_file()
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
