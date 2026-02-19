use axum::{
    extract::{Path, Query, Request},
    http::Method,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, head, post, put},
    Router,
};
use std::sync::Arc;

use crate::s3::{auth, bucket, multipart, object, sts};
use crate::AppState;

/// Simple request logger middleware
async fn log_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::info!(">>> {} {}", method, uri);
    let res = next.run(req).await;
    tracing::info!("<<< {} {} -> {}", method, uri, res.status());
    res
}

/// Build the S3-compatible API router
pub fn build_router(state: AppState) -> Router {
    // STS endpoint (POST /) does NOT go through the SigV4 auth middleware
    // because the console uses STS *to obtain* credentials.
    let sts_router = Router::new()
        .route("/", post(sts::assume_role))
        .layer(middleware::from_fn(log_middleware))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(state.clone());

    // All other S3 routes go through SigV4 auth
    let s3_router = Router::new()
        // Service-level operations
        .route("/", get(bucket::list_buckets))
        // Bucket-level operations
        .route("/:bucket", put(bucket::create_bucket))
        .route("/:bucket/", put(bucket::create_bucket))
        .route("/:bucket", delete(bucket::delete_bucket))
        .route("/:bucket/", delete(bucket::delete_bucket))
        .route("/:bucket", head(bucket::head_bucket))
        .route("/:bucket/", head(bucket::head_bucket))
        .route("/:bucket", get(bucket_or_list_handler))
        .route("/:bucket/", get(bucket_or_list_handler))
        // Object-level operations
        .route("/:bucket/*key", put(object_put_handler))
        .route("/:bucket/*key", get(object::get_object))
        .route("/:bucket/*key", head(object::head_object))
        .route("/:bucket/*key", delete(object_delete_handler))
        .route("/:bucket/*key", post(object_post_handler))
        // Apply SigV4 auth middleware
        .layer(middleware::from_fn(auth::auth_middleware))
        // Inject S3 config into extensions for the auth middleware
        .layer(axum::Extension(Arc::new(state.config.s3.clone())))
        // Apply logger middleware
        .layer(middleware::from_fn(log_middleware))
        // Increase body limit to 5GB
        .layer(axum::extract::DefaultBodyLimit::max(5 * 1024 * 1024 * 1024))
        .with_state(state);

    // Merge routers — STS routes take priority for POST /
    sts_router.merge(s3_router)
}

/// GET /{bucket} — dispatches to ListObjectsV2 or other bucket-level GET
async fn bucket_or_list_handler(
    state: axum::extract::State<AppState>,
    path: Path<String>,
    query: Query<bucket::ListObjectsQuery>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, crate::s3::error::S3Error> {
    // Always treat GET /{bucket} as ListObjectsV2
    bucket::list_objects_v2(state, path, query).await
}

/// PUT /{bucket}/{key} — dispatches to PutObject or UploadPart
async fn object_put_handler(
    state: axum::extract::State<AppState>,
    path: Path<(String, String)>,
    query: Query<multipart::MultipartQuery>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<axum::response::Response, crate::s3::error::S3Error> {
    if query.part_number.is_some() && query.upload_id.is_some() {
        // UploadPart
        multipart::upload_part(state, path, query, body).await
    } else {
        // PutObject
        object::put_object(state, path, headers, body).await
    }
}

/// DELETE /{bucket}/{key} — dispatches to DeleteObject or AbortMultipartUpload
async fn object_delete_handler(
    state: axum::extract::State<AppState>,
    path: Path<(String, String)>,
    query: Query<multipart::MultipartQuery>,
) -> Result<axum::response::Response, crate::s3::error::S3Error> {
    if query.upload_id.is_some() {
        // AbortMultipartUpload
        multipart::abort_multipart_upload(state, path, query).await
    } else {
        // DeleteObject
        object::delete_object(state, path).await
    }
}

/// POST /{bucket}/{key} — dispatches to CreateMultipartUpload or CompleteMultipartUpload
async fn object_post_handler(
    state: axum::extract::State<AppState>,
    path: Path<(String, String)>,
    query: Query<multipart::MultipartQuery>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Result<axum::response::Response, crate::s3::error::S3Error> {
    if query.uploads.is_some() {
        // CreateMultipartUpload
        multipart::create_multipart_upload(state, path, headers).await
    } else if query.upload_id.is_some() {
        // CompleteMultipartUpload
        multipart::complete_multipart_upload(state, path, query, body).await
    } else {
        Err(crate::s3::error::S3Error::InvalidRequest(
            "Invalid POST request".to_string(),
        ))
    }
}
