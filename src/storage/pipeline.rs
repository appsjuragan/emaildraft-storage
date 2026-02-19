use anyhow::{bail, Context, Result};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, ModelTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set,
};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::config::AppConfig;
use crate::db::entities::{bucket, chunk, object};
use crate::email::metadata::ChunkMetadata;
use crate::email::provider::EmailProvider;
use crate::storage::chunker;
use crate::storage::hasher;

/// Orchestrates the full upload/download/delete flow between
/// the S3 API layer, PostgreSQL metadata, and email draft storage.
pub struct StoragePipeline {
    config: AppConfig,
    db: DatabaseConnection,
    email: Arc<dyn EmailProvider>,
    email_account_id: Uuid,
}

impl StoragePipeline {
    pub fn new(
        config: AppConfig,
        db: DatabaseConnection,
        email: Arc<dyn EmailProvider>,
        email_account_id: Uuid,
    ) -> Self {
        Self {
            config,
            db,
            email,
            email_account_id,
        }
    }

    /// Upload an object: buffer → hash → chunk → store as email drafts → record in DB
    /// Implements deduplication: reuses existing "active" chunks if hash matches.
    pub async fn upload(
        &self,
        bucket_id: Uuid,
        key: &str,
        data: &[u8],
        content_type: &str,
        metadata_json: Option<serde_json::Value>,
    ) -> Result<object::Model> {
        let hashes = hasher::compute_hashes(data);
        let etag = format!("\"{}\"", hashes.md5);
        let total_size = data.len() as u64;

        // Chunk the data
        let chunk_size = self.config.chunk_size_bytes();
        let chunks = chunker::chunk_data(data, chunk_size);
        let total_chunks = chunks.len() as u32;

        // Delete existing object if it exists (overwrite semantics)
        self.delete_by_key(bucket_id, key).await.ok();

        // Create object record
        let object_id = Uuid::new_v4();
        let now = Utc::now();

        let obj = object::ActiveModel {
            id: Set(object_id),
            bucket_id: Set(bucket_id),
            key: Set(key.to_string()),
            size: Set(total_size as i64),
            etag: Set(etag.clone()),
            content_type: Set(content_type.to_string()),
            metadata: Set(metadata_json),
            chunk_count: Set(total_chunks as i32),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let obj = obj
            .insert(&self.db)
            .await
            .context("Failed to insert object record")?;

        // Upload each chunk as an email draft
        for chunk_data in &chunks {
            // Deduplication: Check for existing active chunk with same hash
            let existing_chunk = chunk::Entity::find()
                .filter(chunk::Column::Hash.eq(&chunk_data.hash))
                .filter(chunk::Column::Status.eq("active"))
                .one(&self.db)
                .await
                .context("Failed to check for duplicate chunks")?;

            let (draft_uid, is_reused) = if let Some(existing) = existing_chunk {
                tracing::info!(
                    "Deduplication hit: Reusing chunk hash {} (uid {})",
                    chunk_data.hash,
                    existing.draft_uid
                );
                (existing.draft_uid, true)
            } else {
                // Try to recycle a 'free' chunk from the pool
                let free_chunk = chunk::Entity::find()
                    .filter(chunk::Column::Status.eq("free"))
                    .one(&self.db)
                    .await
                    .context("Failed to check for free chunks")?;

                let meta = ChunkMetadata {
                    v: 1,
                    bucket: key.to_string(),
                    key: key.to_string(),
                    chunk_idx: chunk_data.index,
                    total_chunks,
                    object_id: object_id.to_string(),
                    chunk_hash: chunk_data.hash.clone(),
                    total_size,
                    content_type: content_type.to_string(),
                };

                let subject = meta
                    .encode_subject()
                    .context("Failed to encode chunk metadata")?;

                if let Some(free) = free_chunk {
                    tracing::info!("Recycling free chunk slot (old uid {})", free.draft_uid);
                    // To "recycle" in IMAP, we must append new and delete old
                    // (This keeps the total count exactly the same after the operation)
                    let new_uid = self
                        .email
                        .create_draft(&subject, &chunk_data.data)
                        .await
                        .context("Failed to create draft during recycling")?;

                    self.email.delete_draft(free.draft_uid as u32).await.ok(); // Ignore if old one is already gone

                    // Delete the free chunk record so we can create a new active one
                    chunk::Entity::delete_by_id(free.id)
                        .exec(&self.db)
                        .await
                        .ok();

                    (new_uid as i32, false)
                } else {
                    // No existing chunk and no free slots, upload new
                    let new_uid = match self.email.create_draft(&subject, &chunk_data.data).await {
                        Ok(uid) => uid,
                        Err(e) => {
                            return Err(e).context(format!(
                                "Failed to create draft for chunk {}",
                                chunk_data.index
                            ));
                        }
                    };
                    (new_uid as i32, false)
                }
            };

            // Record chunk in DB
            let chunk_record = chunk::ActiveModel {
                id: Set(Uuid::new_v4()),
                object_id: Set(object_id),
                chunk_index: Set(chunk_data.index as i32),
                size: Set(chunk_data.size as i64),
                hash: Set(chunk_data.hash.clone()),
                draft_uid: Set(draft_uid),
                email_account_id: Set(self.email_account_id),
                status: Set("active".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            };

            chunk_record
                .insert(&self.db)
                .await
                .context("Failed to insert chunk record")?;
        }

        tracing::info!(
            "Object '{}' uploaded: {} bytes, {} chunks, ETag: {}",
            key,
            total_size,
            total_chunks,
            etag
        );

        Ok(obj)
    }

    /// Download an object: look up chunks in DB → fetch from email drafts → concatenate
    pub async fn download(&self, object_id: Uuid) -> Result<Vec<u8>> {
        // Get all chunks ordered by index
        let chunks = chunk::Entity::find()
            .filter(chunk::Column::ObjectId.eq(object_id))
            .order_by_asc(chunk::Column::ChunkIndex)
            .all(&self.db)
            .await
            .context("Failed to query chunks")?;

        if chunks.is_empty() {
            bail!("No chunks found for object {}", object_id);
        }

        let mut data = Vec::new();
        for chunk_record in &chunks {
            let chunk_data = self
                .email
                .get_draft(chunk_record.draft_uid as u32)
                .await
                .context(format!(
                    "Failed to fetch draft for chunk {}",
                    chunk_record.chunk_index
                ))?;

            data.extend_from_slice(&chunk_data);

            tracing::debug!(
                "Downloaded chunk {}/{} ({} bytes)",
                chunk_record.chunk_index + 1,
                chunks.len(),
                chunk_data.len()
            );
        }

        Ok(data)
    }

    /// Delete an object: mark chunks as 'deleted' → if no other refs, delete email draft
    pub async fn delete(&self, object_id: Uuid) -> Result<()> {
        let chunks = chunk::Entity::find()
            .filter(chunk::Column::ObjectId.eq(object_id))
            .all(&self.db)
            .await
            .context("Failed to query chunks for deletion")?;

        // Process each chunk
        for chunk_record in &chunks {
            // Check if ANY other active object uses this same hash
            // We need to count usage of this hash where status='active' AND object_id != current
            let usage_count = chunk::Entity::find()
                .filter(chunk::Column::Hash.eq(&chunk_record.hash))
                .filter(chunk::Column::Status.eq("active"))
                .filter(chunk::Column::ObjectId.ne(object_id))
                .count(&self.db)
                .await
                .context("Failed to check chunk usage")?;

            if usage_count > 0 {
                tracing::info!(
                    "Chunk hash {} is used by {} other objects. Preserving draft UID {}.",
                    chunk_record.hash,
                    usage_count,
                    chunk_record.draft_uid
                );
                // Just delete the DB record for this specific object's chunk map
                // (Handled by delete_many below)
                // Last reference. Recycling.
                // Move to recycling object to prevent deletion
                let recycling_object = self.get_or_create_recycling_object().await?;

                let mut free_record: chunk::ActiveModel = chunk_record.clone().into();
                free_record.object_id = Set(recycling_object.id);
                free_record.status = Set("free".to_string());

                // Assign a random/unique chunk index to avoid collision in the recycling bucket
                // (Since we don't care about order for free chunks)
                // Use nanoseconds from epoch as a simple unique-ish ID
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                // Mix with draft_uid to reduce collision chance further
                let unique_index = ((nanos as i32) ^ (chunk_record.draft_uid as i32)).abs();

                free_record.chunk_index = Set(unique_index);
                free_record.updated_at = Set(Utc::now());

                free_record
                    .update(&self.db)
                    .await
                    .context("Failed to move chunk to recycling bin")?;

                tracing::info!("Chunk UID {} moved to free pool", chunk_record.draft_uid);
            }
        }

        // Delete chunk records
        chunk::Entity::delete_many()
            .filter(chunk::Column::ObjectId.eq(object_id))
            .exec(&self.db)
            .await
            .context("Failed to delete chunk records")?;

        // Delete object record
        object::Entity::delete_by_id(object_id)
            .exec(&self.db)
            .await
            .context("Failed to delete object record")?;

        tracing::info!("Object {} deleted", object_id);
        Ok(())
    }

    /// Delete an object by bucket_id and key
    pub async fn delete_by_key(&self, bucket_id: Uuid, key: &str) -> Result<()> {
        let obj = object::Entity::find()
            .filter(object::Column::BucketId.eq(bucket_id))
            .filter(object::Column::Key.eq(key))
            .one(&self.db)
            .await?;

        if let Some(obj) = obj {
            self.delete(obj.id).await?;
        }

        Ok(())
    }

    /// Copy an object (creates new chunks by downloading and re-uploading)
    pub async fn copy(
        &self,
        source_object: &object::Model,
        dest_bucket_id: Uuid,
        dest_key: &str,
    ) -> Result<object::Model> {
        let data = self.download(source_object.id).await?;
        let metadata = source_object.metadata.clone();
        self.upload(
            dest_bucket_id,
            dest_key,
            &data,
            &source_object.content_type,
            metadata,
        )
        .await
    }

    async fn get_or_create_recycling_object(&self) -> Result<object::Model> {
        let bucket_name = "recycling-bin";
        let object_key = format!("free-chunks-{}", self.email_account_id);

        // Check if bucket exists
        let bucket = bucket::Entity::find()
            .filter(bucket::Column::Name.eq(bucket_name))
            .one(&self.db)
            .await?;

        let bucket_id = if let Some(b) = bucket {
            b.id
        } else {
            // Create bucket
            let new_bucket = bucket::ActiveModel {
                id: Set(Uuid::new_v4()),
                name: Set(bucket_name.to_string()),
                owner_id: Set("system".to_string()),
                region: Set("local".to_string()),
                created_at: Set(chrono::Utc::now()),
            };
            let b = new_bucket.insert(&self.db).await?;
            b.id
        };

        // Check if object exists
        let object = object::Entity::find()
            .filter(object::Column::BucketId.eq(bucket_id))
            .filter(object::Column::Key.eq(&object_key))
            .one(&self.db)
            .await?;

        if let Some(o) = object {
            Ok(o)
        } else {
            // Create object
            let new_object = object::ActiveModel {
                id: Set(Uuid::new_v4()),
                bucket_id: Set(bucket_id),
                key: Set(object_key),
                size: Set(0),
                etag: Set("".to_string()),
                content_type: Set("application/octet-stream".to_string()),
                chunk_count: Set(0),
                created_at: Set(chrono::Utc::now()),
                updated_at: Set(chrono::Utc::now()),
                ..Default::default()
            };
            Ok(new_object.insert(&self.db).await?)
        }
    }
}
