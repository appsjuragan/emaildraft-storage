use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::entities::{bucket, object};
use crate::s3::error::S3Error;
use crate::s3::xml;
use crate::AppState;

/// PUT /{bucket} — Create bucket
pub async fn create_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    body: axum::body::Bytes,
) -> Result<Response, S3Error> {
    tracing::info!("Creating bucket: {}", bucket_name);
    // Validate bucket name (basic S3 rules)
    if bucket_name.len() < 3 || bucket_name.len() > 63 {
        return Err(S3Error::InvalidBucketName(
            "Bucket name must be between 3 and 63 characters".to_string(),
        ));
    }

    // Check if bucket already exists
    let existing = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    if existing.is_some() {
        return Err(S3Error::BucketAlreadyOwnedByYou(format!(
            "Bucket '{}' already exists",
            bucket_name
        )));
    }

    // Parse optional location constraint from body
    let region = if !body.is_empty() {
        let body_str = std::str::from_utf8(&body).unwrap_or("");
        xml::from_xml::<xml::CreateBucketConfiguration>(body_str)
            .ok()
            .and_then(|c| c.location_constraint)
            .unwrap_or_else(|| state.config.s3.region.clone())
    } else {
        state.config.s3.region.clone()
    };

    let new_bucket = bucket::ActiveModel {
        id: Set(Uuid::new_v4()),
        name: Set(bucket_name.clone()),
        owner_id: Set(state.config.s3.access_key_id.clone()),
        region: Set(region),
        created_at: Set(Utc::now()),
    };

    new_bucket
        .insert(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    tracing::info!("Bucket '{}' created", bucket_name);

    Ok((StatusCode::OK, [("Location", format!("/{}", bucket_name))]).into_response())
}

/// DELETE /{bucket} — Delete bucket
pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    // Check if bucket is empty
    let object_count = object::Entity::find()
        .filter(object::Column::BucketId.eq(bucket.id))
        .count(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    if object_count > 0 {
        return Err(S3Error::BucketNotEmpty(format!(
            "Bucket '{}' is not empty ({} objects)",
            bucket_name, object_count
        )));
    }

    bucket::Entity::delete_by_id(bucket.id)
        .exec(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    tracing::info!("Bucket '{}' deleted", bucket_name);
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// HEAD /{bucket} — Check bucket exists
pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Response, S3Error> {
    let _bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    Ok((
        StatusCode::OK,
        [("x-amz-bucket-region", state.config.s3.region.as_str())],
    )
        .into_response())
}

/// GET / — List all buckets
pub async fn list_buckets(State(state): State<AppState>) -> Result<Response, S3Error> {
    let buckets = bucket::Entity::find()
        .all(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    let result = xml::ListAllMyBucketsResult {
        buckets: xml::BucketsList {
            buckets: buckets
                .iter()
                .map(|b| xml::BucketInfo {
                    name: b.name.clone(),
                    creation_date: b.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                })
                .collect(),
        },
        owner: xml::Owner {
            id: state.config.s3.access_key_id.clone(),
            display_name: state.config.s3.access_key_id.clone(),
        },
    };

    let xml_body = xml::to_xml(&result).map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        xml_body,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListObjectsQuery {
    #[serde(rename = "list-type")]
    pub list_type: Option<String>,
    pub prefix: Option<String>,
    pub delimiter: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<i32>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
    #[serde(rename = "start-after")]
    pub start_after: Option<String>,
    #[serde(rename = "fetch-owner")]
    pub fetch_owner: Option<String>,
    #[serde(rename = "encoding-type")]
    pub encoding_type: Option<String>,
}

/// GET /{bucket}?list-type=2 — List objects in bucket
pub async fn list_objects_v2(
    State(state): State<AppState>,
    Path(bucket_name): Path<String>,
    Query(params): Query<ListObjectsQuery>,
) -> Result<Response, S3Error> {
    let bucket = bucket::Entity::find()
        .filter(bucket::Column::Name.eq(&bucket_name))
        .one(&state.db)
        .await
        .map_err(|e| S3Error::InternalError(e.to_string()))?
        .ok_or_else(|| S3Error::NoSuchBucket(format!("Bucket '{}' not found", bucket_name)))?;

    let prefix = params.prefix.unwrap_or_default();
    let delimiter = params.delimiter.clone();
    let max_keys = params.max_keys.unwrap_or(1000);

    // Query objects with prefix filter
    let mut query = object::Entity::find().filter(object::Column::BucketId.eq(bucket.id));

    if !prefix.is_empty() {
        query = query.filter(object::Column::Key.starts_with(&prefix));
    }

    let objects = query
        .all(&state.db)
        .await
        .map_err(|e: sea_orm::DbErr| S3Error::InternalError(e.to_string()))?;

    // Apply delimiter logic for common prefixes
    let mut contents = Vec::new();
    let mut common_prefixes_set = std::collections::BTreeSet::new();

    for obj in &objects {
        if let Some(ref delim) = delimiter {
            let after_prefix = &obj.key[prefix.len()..];
            if let Some(pos) = after_prefix.find(delim.as_str()) {
                // This key has a delimiter after the prefix — add as common prefix
                let common_prefix = format!("{}{}", prefix, &after_prefix[..=pos]);
                common_prefixes_set.insert(common_prefix);
                continue;
            }
        }

        contents.push(xml::ObjectInfo {
            key: obj.key.clone(),
            last_modified: obj.updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            etag: obj.etag.clone(),
            size: obj.size,
            storage_class: "STANDARD".to_string(),
        });
    }

    // Truncate to max_keys
    let is_truncated = contents.len() as i32 > max_keys;
    let key_count = std::cmp::min(contents.len() as i32, max_keys);
    contents.truncate(max_keys as usize);

    let result = xml::ListBucketResult {
        name: bucket_name,
        prefix,
        delimiter,
        max_keys,
        is_truncated,
        key_count,
        contents,
        common_prefixes: common_prefixes_set
            .into_iter()
            .map(|p| xml::CommonPrefix { prefix: p })
            .collect(),
        continuation_token: params.continuation_token,
        next_continuation_token: None,
    };

    let xml_body = xml::to_xml(&result).map_err(|e| S3Error::InternalError(e.to_string()))?;

    Ok((
        StatusCode::OK,
        [("Content-Type", "application/xml")],
        xml_body,
    )
        .into_response())
}
