use std::sync::Arc;

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, aws::AmazonS3Builder, path::Path};
use thiserror::Error;

use crate::config::Config;

#[derive(Clone)]
pub struct MediaStore {
    store: Arc<dyn ObjectStore>,
}

#[derive(Debug, Error)]
pub enum MediaStoreError {
    #[error("OCI Object Storage configuration is incomplete")]
    NotConfigured,
    #[error("OCI Object Storage configuration is invalid: {0}")]
    InvalidConfiguration(#[source] object_store::Error),
    #[error("OCI Object Storage operation failed: {0}")]
    Operation(#[from] object_store::Error),
}

impl MediaStore {
    pub fn from_config(config: &Config) -> Result<Self, MediaStoreError> {
        let endpoint = config
            .object_storage_endpoint
            .as_ref()
            .ok_or(MediaStoreError::NotConfigured)?;
        let region = config
            .object_storage_region
            .as_ref()
            .ok_or(MediaStoreError::NotConfigured)?;
        let bucket = config
            .object_storage_bucket
            .as_ref()
            .ok_or(MediaStoreError::NotConfigured)?;
        let access_key = config
            .object_storage_access_key_id
            .as_ref()
            .ok_or(MediaStoreError::NotConfigured)?;
        let secret_key = config
            .object_storage_secret_access_key
            .as_ref()
            .ok_or(MediaStoreError::NotConfigured)?;

        let store = AmazonS3Builder::new()
            .with_endpoint(endpoint)
            .with_region(region)
            .with_bucket_name(bucket)
            .with_access_key_id(access_key.expose())
            .with_secret_access_key(secret_key.expose())
            .with_virtual_hosted_style_request(false)
            .build()
            .map_err(MediaStoreError::InvalidConfiguration)?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    pub async fn put_document(
        &self,
        content_sha256: &str,
        filename: &str,
        bytes: &[u8],
    ) -> Result<String, MediaStoreError> {
        let extension =
            crate::document::supported_extension(filename).ok_or(MediaStoreError::NotConfigured)?;
        let key = format!("documents/{content_sha256}.{extension}");
        self.store
            .put(
                &Path::from(key.as_str()),
                PutPayload::from(Bytes::copy_from_slice(bytes)),
            )
            .await?;
        Ok(key)
    }

    #[cfg(test)]
    fn with_store(store: Arc<dyn ObjectStore>) -> Self {
        Self { store }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use object_store::memory::InMemory;

    use super::*;

    fn config() -> Config {
        Config::from_map(HashMap::from([
            ("DATABASE_URL".into(), "postgres://localhost/agora".into()),
            ("WHATSAPP_VERIFY_TOKEN".into(), "verify".into()),
            ("WHATSAPP_APP_SECRET".into(), "secret".into()),
        ]))
        .unwrap()
    }

    #[test]
    fn requires_complete_object_storage_configuration() {
        assert!(matches!(
            MediaStore::from_config(&config()),
            Err(MediaStoreError::NotConfigured)
        ));
    }

    #[tokio::test]
    async fn stores_documents_under_a_content_addressed_key() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let media = MediaStore::with_store(store.clone());

        let key = media
            .put_document("abc123", "Informe.PDF", b"document")
            .await
            .unwrap();

        assert_eq!(key, "documents/abc123.pdf");
        let content = store
            .get(&Path::from(key))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(content, b"document".as_slice());
    }
}
