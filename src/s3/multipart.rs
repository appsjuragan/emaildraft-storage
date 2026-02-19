use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::entities::{bucket, multipart_part, multipart_upload, object};
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::storage::hasher;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct MultipartQuery {
    pub uploads: Option<String>,
    #[serde(rename = "uploadId")]
    pub upload_id: Option<String>,
    #[serde(rename = "partNumber")]
    pub part_number: Option<i32>,
}

/// POST /{bucket}/{key}?uploads — Initiate multipart upload
pub async fn create_multipart_upload(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    // Extract user metadata
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

    let upload_id = Uuid::new_v4();

    let upload = multipart_upload::ActiveModel {
        id: Set(upload_id),
        bucket_id: Set(bucket.id),
        key: Set(key.clone()),
        content_type: Set(Some(content_type)),
        metadata: Set(metadata_json),
        created_at: Set(Utc::now()),
    };

    upload
        .insert(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    let result = xml::InitiateMultipartUploadResult {
        bucket: bucket_name,
        key,
        upload_id: upload_id.to_string(),
    };

    let xml_body = xml::to_xml(&result).map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        xml_body,
    )
        .into_response())
}

/// PUT /{bucket}/{key}?partNumber={n}&uploadId={id} — Upload part
pub async fn upload_part(
    State(state): State<AppState>,
    Path((_bucket_name, _key)): Path<(String, String)>,
    Query(params): Query<MultipartQuery>,
    body: axum::body::Bytes,
) -> Result<Response, S3Error> {
    let upload_id = params
        .upload_id
        .as_ref()
        .ok_or_else(|| S3Error::InvalidArgument("Missing uploadId".to_string()))?;

    let part_number = params
        .part_number
        .ok_or_else(|| S3Error::InvalidArgument("Missing partNumber".to_string()))?;

    let upload_uuid = Uuid::parse_str(upload_id)
        .map_err(|_| S3Error::NoSuchUpload("Invalid upload ID".to_string()))?;

    // Verify upload exists
    let _upload = multipart_upload::Entity::find_by_id(upload_uuid)
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchUpload(format!("Upload '{}' not found", upload_id)))?;

    // Compute ETag (MD5 of part data)
    let etag = format!("\"{}\"", hasher::compute_md5(&body));

    // Save part data to temp file
    let temp_dir = &state.config.storage.temp_dir;
    tokio::fs::create_dir_all(temp_dir)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    let temp_path = temp_dir.join(format!("{}-{}", upload_id, part_number));
    tokio::fs::write(&temp_path, &body)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    // Upsert part record
    // Delete existing if exists (overwrite semantics for same part number)
    multipart_part::Entity::delete_many()
        .filter(multipart_part::Column::UploadId.eq(upload_uuid))
        .filter(multipart_part::Column::PartNumber.eq(part_number))
        .exec(&state.db)
        .await
        .ok();

    let part = multipart_part::ActiveModel {
        id: Set(Uuid::new_v4()),
        upload_id: Set(upload_uuid),
        part_number: Set(part_number),
        size: Set(body.len() as i64),
        etag: Set(etag.clone()),
        temp_path: Set(Some(temp_path.to_string_lossy().to_string())),
        created_at: Set(Utc::now()),
    };

    part.insert(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    tracing::info!(
        "Part {} of upload {} received ({} bytes)",
        part_number,
        upload_id,
        body.len()
    );

    Ok((StatusCode::OK, [("ETag", etag.as_str())]).into_response())
}

/// POST /{bucket}/{key}?uploadId={id} — Complete multipart upload
pub async fn complete_multipart_upload(
    State(state): State<AppState>,
    Path((bucket_name, key)): Path<(String, String)>,
    Query(params): Query<MultipartQuery>,
    body: axum::body::Bytes,
) -> Result<Response, S3Error> {
    let upload_id = params
        .upload_id
        .as_ref()
        .ok_or_else(|| S3Error::InvalidArgument("Missing uploadId".to_string()))?;

    let upload_uuid = Uuid::parse_str(upload_id)
        .map_err(|_| S3Error::NoSuchUpload("Invalid upload ID".to_string()))?;

    let upload = multipart_upload::Entity::find_by_id(upload_uuid)
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchUpload(format!("Upload '{}' not found", upload_id)))?;

    // Parse the CompleteMultipartUpload XML request
    let body_str = std::str::from_utf8(&body)
        .map_err(|_| S3Error::MalformedXML("Invalid UTF-8 in request body".to_string()))?;

    let complete_request: xml::CompleteMultipartUploadRequest =
        xml::from_xml(body_str).map_err(|e| {
            S3Error::MalformedXML(format!(
                "Failed to parse CompleteMultipartUpload XML: {}",
                e
            ))
        })?;

    // Verify parts are in ascending order
    for window in complete_request.parts.windows(2) {
        if window[0].part_number >= window[1].part_number {
            return Err(S3Error::InvalidPartOrder(
                "Parts must be in ascending order".to_string(),
            ));
        }
    }

    // Get all stored parts ordered by part number
    let stored_parts = multipart_part::Entity::find()
        .filter(multipart_part::Column::UploadId.eq(upload_uuid))
        .order_by_asc(multipart_part::Column::PartNumber)
        .all(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    // Concatenate all part data
    let mut combined_data = Vec::new();
    for requested_part in &complete_request.parts {
        let stored = stored_parts
            .iter()
            .find(|p| p.part_number == requested_part.part_number)
            .ok_or_else(|| {
                S3Error::InvalidPart(format!("Part {} not found", requested_part.part_number))
            })?;

        let temp_path = stored.temp_path.as_ref().ok_or_else(|| {
            S3Error::InternalError(format!("No temp path for part {}", stored.part_number))
        })?;

        let part_data = tokio::fs::read(temp_path)
            .await
            .map_err(|e| S3Error::InternalError(format!("Failed to read part data: {}", e)))?;

        combined_data.extend_from_slice(&part_data);
    }

    // Get the bucket
    let _bucket = bucket::Entity::find_by_id(upload.bucket_id)
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket("Bucket not found".to_string()))?;

    // Upload the combined data via storage pipeline
    let content_type = upload
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let pipeline = state.pipeline.lock().await;
    let obj = pipeline
        .upload(
            upload.bucket_id,
            &upload.key,
            &combined_data,
            &content_type,
            upload.metadata,
        )
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?;

    drop(pipeline);

    // Cleanup: delete temp files and DB records
    for stored in &stored_parts {
        if let Some(ref temp_path) = stored.temp_path {
            tokio::fs::remove_file(temp_path).await.ok();
        }
    }

    multipart_part::Entity::delete_many()
        .filter(multipart_part::Column::UploadId.eq(upload_uuid))
        .exec(&state.db)
        .await
        .ok();

    multipart_upload::Entity::delete_by_id(upload_uuid)
        .exec(&state.db)
        .await
        .ok();

    let result = xml::CompleteMultipartUploadResult {
        location: format!("/{}/{}", bucket_name, key),
        bucket: bucket_name,
        key,
        etag: obj.etag.clone(),
    };

    let xml_body = xml::to_xml(&result).map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        xml_body,
    )
        .into_response())
}

/// DELETE /{bucket}/{key}?uploadId={id} — Abort multipart upload
pub async fn abort_multipart_upload(
    State(state): State<AppState>,
    Path((_bucket_name, _key)): Path<(String, String)>,
    Query(params): Query<MultipartQuery>,
) -> Result<Response, S3Error> {
    let upload_id = params
        .upload_id
        .as_ref()
        .ok_or_else(|| S3Error::InvalidArgument("Missing uploadId".to_string()))?;

    let upload_uuid = Uuid::parse_str(upload_id)
        .map_err(|_| S3Error::NoSuchUpload("Invalid upload ID".to_string()))?;

    // Delete temp files
    let parts = multipart_part::Entity::find()
        .filter(multipart_part::Column::UploadId.eq(upload_uuid))
        .all(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    for part in &parts {
        if let Some(ref temp_path) = part.temp_path {
            tokio::fs::remove_file(temp_path).await.ok();
        }
    }

    // Cleanup DB
    multipart_part::Entity::delete_many()
        .filter(multipart_part::Column::UploadId.eq(upload_uuid))
        .exec(&state.db)
        .await
        .ok();

    multipart_upload::Entity::delete_by_id(upload_uuid)
        .exec(&state.db)
        .await
        .ok();

    Ok(StatusCode::NO_CONTENT.into_response())
}
