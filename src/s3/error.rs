use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// S3-compatible error types
#[derive(Debug, Clone)]
pub enum S3Error {
    AccessDenied(String),
    NoSuchBucket(String),
    NoSuchKey(String),
    BucketAlreadyOwnedByYou(String),
    BucketAlreadyExists(String),
    BucketNotEmpty(String),
    InvalidArgument(String),
    InvalidBucketName(String),
    InvalidPart(String),
    InvalidPartOrder(String),
    NoSuchUpload(String),
    MalformedXML(String),
    InternalError(String),
    MissingContentLength,
    SignatureDoesNotMatch(String),
    InvalidRequest(String),
}

impl S3Error {
    fn code(&self) -> &str {
        match self {
            S3Error::AccessDenied(_) => "AccessDenied",
            S3Error::NoSuchBucket(_) => "NoSuchBucket",
            S3Error::NoSuchKey(_) => "NoSuchKey",
            S3Error::BucketAlreadyOwnedByYou(_) => "BucketAlreadyOwnedByYou",
            S3Error::BucketAlreadyExists(_) => "BucketAlreadyExists",
            S3Error::BucketNotEmpty(_) => "BucketNotEmpty",
            S3Error::InvalidArgument(_) => "InvalidArgument",
            S3Error::InvalidBucketName(_) => "InvalidBucketName",
            S3Error::InvalidPart(_) => "InvalidPart",
            S3Error::InvalidPartOrder(_) => "InvalidPartOrder",
            S3Error::NoSuchUpload(_) => "NoSuchUpload",
            S3Error::MalformedXML(_) => "MalformedXML",
            S3Error::InternalError(_) => "InternalError",
            S3Error::MissingContentLength => "MissingContentLength",
            S3Error::SignatureDoesNotMatch(_) => "SignatureDoesNotMatch",
            S3Error::InvalidRequest(_) => "InvalidRequest",
        }
    }

    fn status_code(&self) -> StatusCode {
        match self {
            S3Error::AccessDenied(_) | S3Error::SignatureDoesNotMatch(_) => StatusCode::FORBIDDEN,
            S3Error::NoSuchBucket(_) | S3Error::NoSuchKey(_) => StatusCode::NOT_FOUND,
            S3Error::BucketAlreadyOwnedByYou(_)
            | S3Error::BucketAlreadyExists(_)
            | S3Error::BucketNotEmpty(_) => StatusCode::CONFLICT,
            S3Error::InvalidArgument(_)
            | S3Error::InvalidBucketName(_)
            | S3Error::InvalidPart(_)
            | S3Error::InvalidPartOrder(_)
            | S3Error::MalformedXML(_)
            | S3Error::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            S3Error::NoSuchUpload(_) => StatusCode::NOT_FOUND,
            S3Error::MissingContentLength => StatusCode::LENGTH_REQUIRED,
            S3Error::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> &str {
        match self {
            S3Error::AccessDenied(m) => m,
            S3Error::NoSuchBucket(m) => m,
            S3Error::NoSuchKey(m) => m,
            S3Error::BucketAlreadyOwnedByYou(m) => m,
            S3Error::BucketAlreadyExists(m) => m,
            S3Error::BucketNotEmpty(m) => m,
            S3Error::InvalidArgument(m) => m,
            S3Error::InvalidBucketName(m) => m,
            S3Error::InvalidPart(m) => m,
            S3Error::InvalidPartOrder(m) => m,
            S3Error::NoSuchUpload(m) => m,
            S3Error::MalformedXML(m) => m,
            S3Error::InternalError(m) => m,
            S3Error::MissingContentLength => "Missing Content-Length header",
            S3Error::SignatureDoesNotMatch(m) => m,
            S3Error::InvalidRequest(m) => m,
        }
    }

    fn to_xml(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>{}</Code>
  <Message>{}</Message>
  <RequestId>{}</RequestId>
</Error>"#,
            self.code(),
            self.message(),
            uuid::Uuid::new_v4()
        )
    }
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let xml = self.to_xml();
        (status, [("Content-Type", "application/xml")], xml).into_response()
    }
}

impl From<anyhow::Error> for S3Error {
    fn from(err: anyhow::Error) -> Self {
        S3Error::InternalError(err.to_string())
    }
}
