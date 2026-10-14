//! The S3-compatible driver (feature `s3`): a private bucket through `object_store`.

use super::*;
use futures_util::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt};

/// A private S3-compatible bucket.
pub struct S3Storage {
    store: Arc<dyn ObjectStore>,
}

impl S3Storage {
    pub fn new(config: &S3Config) -> Result<Self, StorageError> {
        if config.bucket.is_empty() {
            return Err(StorageError::Backend("`storage.s3.bucket` is required".into()));
        }
        let mut builder = AmazonS3Builder::from_env()
            .with_bucket_name(&config.bucket)
            .with_region(config.region.clone().unwrap_or_else(|| "us-east-1".into()))
            .with_allow_http(config.allow_http);
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint).with_virtual_hosted_style_request(false);
        }
        let store = builder.build().map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(Self { store: Arc::new(store) })
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
        std::env::temp_dir()
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
}
