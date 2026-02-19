use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use crate::db::entities::{bucket, object};
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::AppState;

/// PUT /{bucket}/{key..} — Upload object
pub async fn put_object(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, S3Error> {
    // Check for copy source header (CopyObject)
    if let Some(copy_source) = headers.get("x-amz-copy-source") {
        return copy_object(state, &bucket_name, &key, copy_source, &headers).await;
    }

    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    // Extract content type
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // Extract user metadata (x-amz-meta-*)
    let mut user_metadata = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if name_str.starts_with("x-amz-meta-") {
            let meta_key = name_str.strip_prefix("x-amz-meta-").unwrap();
            if let Ok(val) = value.to_str() {
                user_metadata.insert(
                    meta_key.to_string(),
                    serde_json::Value::String(val.to_string()),
                );
            }
        }
    }

    let metadata_json = if user_metadata.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(user_metadata))
    };

    // Upload via storage pipeline
    let pipeline = state.pipeline.lock().await;
    let obj = pipeline
        .upload(bucket.id, &key, &body, &content_type, metadata_json)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [
            ("ETag", obj.etag.as_str()),
            ("x-amz-request-id", &uuid::Uuid::new_v4().to_string()),
        ],
    )
        .into_response())
}

/// GET /{bucket}/{key..} — Download object
pub async fn get_object(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    let obj = object::Entity::find()
        .filter(object::Column::BucketId.eq(bucket.id))
        .filter(object::Column::Key.eq(&key))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchKey(format!("Object '{}' not found", key)))?;

    // Download via storage pipeline
    let pipeline = state.pipeline.lock().await;
    let data = pipeline
        .download(obj.id)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &obj.content_type)
        .header("Content-Length", obj.size.to_string())
        .header("ETag", &obj.etag)
        .header(
            "Last-Modified",
            obj.updated_at
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string(),
        )
        .header("x-amz-request-id", uuid::Uuid::new_v4().to_string());

    // Add user metadata headers
    if let Some(ref metadata) = obj.metadata {
        if let Some(map) = metadata.as_object() {
            for (k, v) in map {
                if let Some(val) = v.as_str() {
                    response = response.header(format!("x-amz-meta-{}", k), val);
                }
            }
        }
    }

    response
        .body(Body::from(data))
        .map_err(|e| S3Error::InternalError(e.to_string()))
}

/// HEAD /{bucket}/{key..} — Object metadata
pub async fn head_object(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    let obj = object::Entity::find()
        .filter(object::Column::BucketId.eq(bucket.id))
        .filter(object::Column::Key.eq(&key))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchKey(format!("Object '{}' not found", key)))?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", &obj.content_type)
        .header("Content-Length", obj.size.to_string())
        .header("ETag", &obj.etag)
        .header(
            "Last-Modified",
            obj.updated_at
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string(),
        )
        .header("Accept-Ranges", "bytes")
        .header("x-amz-request-id", uuid::Uuid::new_v4().to_string());

    // Add user metadata headers
    if let Some(ref metadata) = obj.metadata {
        if let Some(map) = metadata.as_object() {
            for (k, v) in map {
                if let Some(val) = v.as_str() {
                    response = response.header(format!("x-amz-meta-{}", k), val);
                }
            }
        }
    }

    response
        .body(Body::empty())
        .map_err(|e| S3Error::InternalError(e.to_string()))
}

/// DELETE /{bucket}/{key..} — Delete object
pub async fn delete_object(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    // Delete via pipeline (handles draft cleanup)
    let pipeline = state.pipeline.lock().await;
    pipeline
        .delete_by_key(bucket.id, &key)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Internal: CopyObject (PUT with x-amz-copy-source header)
async fn copy_object(
    state: AppState,
    dest_bucket_name: &str,
    dest_key: &str,
    copy_source: &HeaderValue,
    _headers: &HeaderMap,
) -> Result<Response, S3Error> {
    let source_path = copy_source
        .to_str()
        .map_err(|_| S3Error::InvalidArgument("Invalid x-amz-copy-source".to_string()))?;

    // Parse source: /bucket/key or bucket/key
    let source_path = source_path.strip_prefix('/').unwrap_or(source_path);
    let (source_bucket_name, source_key) = source_path
        .split_once('/')
        .ok_or_else(|| S3Error::InvalidArgument("Invalid copy source format".to_string()))?;

    // Find source bucket and object
    let source_bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(source_bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| {
            S3Error::NoSuchBucket(format!("Source bucket '{}' not found", source_bucket_name))
        })?;

    let source_object = object::Entity::find()
        .filter(object::Column::BucketId.eq(source_bucket.id))
        .filter(object::Column::Key.eq(source_key))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchKey(format!("Source object '{}' not found", source_key)))?;

    // Find destination bucket
    let dest_bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(dest_bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| {
            S3Error::NoSuchBucket(format!(
                "Destination bucket '{}' not found",
                dest_bucket_name
            ))
        })?;

    // Copy via pipeline
    let pipeline = state.pipeline.lock().await;
    let new_obj = pipeline
        .copy(&source_object, dest_bucket.id, dest_key)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    let result = xml::CopyObjectResult {
        last_modified: new_obj
            .updated_at
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        etag: new_obj.etag.clone(),
    };

    let xml_body = xml::to_xml(&result).map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        xml_body,
    )
        .into_response())
}
