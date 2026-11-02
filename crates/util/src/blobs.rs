//! Where uploaded media lives: an S3 bucket, or memory for tests. Blobs are keyed
//! by path, and read back in whole or by byte range.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use anyhow::Result;
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use object_store::aws::AmazonS3Builder;
use object_store::memory::InMemory;
use object_store::path::Path;
use object_store::{GetOptions, GetRange, ObjectStore, PutPayload};
use tokio::sync::{Mutex, MutexGuard};

/// How many locks the keys share.
const LOCKS: usize = 64;

/// A stream of bytes being read.
pub type ByteStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = std::io::Result<Bytes>> + Send>>;

/// The bucket media is stored in.
#[derive(Clone, Debug, Default)]
pub struct S3Config {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
}

#[derive(Clone)]
pub struct Blobs {
    store: Arc<dyn ObjectStore>,
    locks: Arc<[Mutex<()>]>,
}

/// A blob being read: its whole size, and the bytes asked for.
pub struct BlobRead {
    pub size: u64,
    pub range: Range<u64>,
    pub stream: ByteStream,
}

impl Blobs {
    fn new(store: Arc<dyn ObjectStore>) -> Self {
        Self {
            store,
            locks: (0..LOCKS).map(|_| Mutex::new(())).collect(),
        }
    }

    pub fn s3(config: &S3Config) -> Result<Self> {
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(&config.bucket)
            .with_region(&config.region)
            .with_access_key_id(&config.access_key)
            .with_secret_access_key(&config.secret_key);
        if !config.endpoint.is_empty() {
            builder = builder
                .with_endpoint(&config.endpoint)
                .with_allow_http(config.endpoint.starts_with("http://"));
        }

        Ok(Self::new(Arc::new(builder.build()?)))
    }

    pub fn memory() -> Self {
        Self::new(Arc::new(InMemory::new()))
    }

    /// Hold `key` in this process until the guard drops, for a writer that changes
    /// a blob and the record of it together. Unrelated keys may share a lock.
    pub async fn lock(&self, key: &str) -> MutexGuard<'_, ()> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        self.locks[hasher.finish() as usize % LOCKS].lock().await
    }

    pub async fn put(&self, key: &str, bytes: Bytes) -> Result<()> {
        self.store
            .put(&Path::from(key), PutPayload::from(bytes))
            .await?;
        Ok(())
    }

    /// A blob's bytes, all of them or the range asked for, or `None` if there is
    /// no such blob.
    pub async fn get(&self, key: &str, range: Option<Range<u64>>) -> Result<Option<BlobRead>> {
        let options = GetOptions {
            range: range.map(GetRange::Bounded),
            ..Default::default()
        };

        match self.store.get_opts(&Path::from(key), options).await {
            Ok(result) => Ok(Some(BlobRead {
                size: result.meta.size,
                range: result.range.clone(),
                stream: result.into_stream().map_err(std::io::Error::other).boxed(),
            })),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// A blob's size, or `None` if there is no such blob.
    pub async fn size(&self, key: &str) -> Result<Option<u64>> {
        match self.store.head(&Path::from(key)).await {
            Ok(meta) => Ok(Some(meta.size)),
            Err(object_store::Error::NotFound { .. }) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        match self.store.delete(&Path::from(key)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Delete every blob under a prefix.
    pub async fn delete_prefix(&self, prefix: &str) -> Result<()> {
        let prefix = Path::from(prefix);
        let paths = self
            .store
            .list(Some(&prefix))
            .map_ok(|meta| meta.location)
            .boxed();
        self.store
            .delete_stream(paths)
            .try_collect::<Vec<_>>()
            .await?;
        Ok(())
    }
}

#[cfg(feature = "testing")]
pub mod testing {
    //! Blobs in a directory, where a directory in a blob's place makes its delete
    //! fail, as a bucket that refuses one does.

    use std::sync::Arc;

    use anyhow::Result;
    use object_store::local::LocalFileSystem;

    use super::Blobs;

    /// Blobs kept under `dir`, which is created.
    pub fn local(dir: &str) -> Result<Blobs> {
        std::fs::create_dir_all(dir)?;
        Ok(Blobs::new(Arc::new(LocalFileSystem::new_with_prefix(dir)?)))
    }
}
