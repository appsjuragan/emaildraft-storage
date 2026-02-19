use axum::{
    extract::Request,
    http::{HeaderMap, Method},
    middleware::Next,
    response::Response,
};
use chrono::{NaiveDateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::config::S3Config;
use crate::s3::error::S3Error;

type HmacSha256 = Hmac<Sha256>;

/// Extract S3 auth components from the Authorization header
struct AuthInfo {
    access_key_id: String,
    date: String,
    region: String,
    signed_headers: Vec<String>,
    signature: String,
}

fn parse_authorization(header: &str) -> Option<AuthInfo> {
    // Format: AWS4-HMAC-SHA256 Credential=<key>/<date>/<region>/s3/aws4_request,
    //         SignedHeaders=<headers>, Signature=<sig>
    let header = header.strip_prefix("AWS4-HMAC-SHA256 ")?;

    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;

    for part in header.split(", ") {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("Credential=") {
            credential = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("SignedHeaders=") {
            signed_headers = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("Signature=") {
            signature = Some(val.to_string());
        }
    }

    let credential = credential?;
    let parts: Vec<&str> = credential.splitn(5, '/').collect();
    if parts.len() < 5 {
        return None;
    }

    Some(AuthInfo {
        access_key_id: parts[0].to_string(),
        date: parts[1].to_string(),
        region: parts[2].to_string(),
        signed_headers: signed_headers?.split(';').map(|s| s.to_string()).collect(),
        signature: signature?,
    })
}

/// Derive AWS SigV4 signing key
fn derive_signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_secret = format!("AWS4{}", secret);

    let mut mac = HmacSha256::new_from_slice(k_secret.as_bytes()).unwrap();
    mac.update(date.as_bytes());
    let k_date = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&k_date).unwrap();
    mac.update(region.as_bytes());
    let k_region = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&k_region).unwrap();
    mac.update(b"s3");
    let k_service = mac.finalize().into_bytes();

    let mut mac = HmacSha256::new_from_slice(&k_service).unwrap();
    mac.update(b"aws4_request");
    mac.finalize().into_bytes().to_vec()
}

/// Compute HMAC-SHA256 signature
fn compute_signature(signing_key: &[u8], string_to_sign: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(signing_key).unwrap();
    mac.update(string_to_sign.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Build canonical request string
fn build_canonical_request(
    method: &Method,
    uri_path: &str,
    query_string: &str,
    headers: &HeaderMap,
    signed_headers: &[String],
    payload_hash: &str,
) -> String {
    // Canonical URI
    let canonical_uri = if uri_path.is_empty() {
        "/".to_string()
    } else {
        uri_path.to_string()
    };

    // Canonical query string (sorted by param name)
    let canonical_query = if query_string.is_empty() {
        String::new()
    } else {
        let mut params: Vec<(&str, &str)> = query_string
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|param| {
                let mut parts = param.splitn(2, '=');
                let key = parts.next().unwrap_or("");
                let val = parts.next().unwrap_or("");
                (key, val)
            })
            .collect();
        params.sort_by_key(|(k, _)| *k);
        params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&")
    };

    // Canonical headers
    let canonical_headers: String = signed_headers
        .iter()
        .map(|name| {
            let value = headers
                .get(name.as_str())
                .map(|v| v.to_str().unwrap_or("").trim().to_string())
                .unwrap_or_default();
            format!("{}:{}\n", name, value)
        })
        .collect();

    let signed_headers_str = signed_headers.join(";");

    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, canonical_uri, canonical_query, canonical_headers, signed_headers_str, payload_hash
    )
}

/// AWS SigV4 authentication middleware for axum
pub async fn auth_middleware(request: Request, next: Next) -> Result<Response, S3Error> {
    // Get config from extensions
    let config = request
        .extensions()
        .get::<Arc<S3Config>>()
        .cloned()
        .ok_or_else(|| S3Error::InternalError("Missing S3 config".to_string()))?;

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // If no auth header, allow for now (some S3 operations like health checks)
    if auth_header.is_empty() {
        return Ok(next.run(request).await);
    }

    // Parse the auth header
    let auth_info = parse_authorization(auth_header)
        .ok_or_else(|| S3Error::AccessDenied("Invalid Authorization header format".to_string()))?;

    // Verify access key
    if auth_info.access_key_id != config.access_key_id {
        return Err(S3Error::AccessDenied(
            "The AWS Access Key Id you provided does not exist in our records".to_string(),
        ));
    }

    // Check timestamp (15-minute skew tolerance)
    let amz_date = request
        .headers()
        .get("x-amz-date")
        .and_then(|v| v.to_str().ok())
        .or_else(|| request.headers().get("date").and_then(|v| v.to_str().ok()))
        .unwrap_or("");

    if !amz_date.is_empty() {
        if let Ok(request_time) = NaiveDateTime::parse_from_str(amz_date, "%Y%m%dT%H%M%SZ") {
            let now = Utc::now().naive_utc();
            let diff = (now - request_time).num_seconds().abs();
            if diff > 900 {
                return Err(S3Error::AccessDenied(
                    "Request timestamp is too skewed".to_string(),
                ));
            }
        }
    }

    // Get payload hash
    let payload_hash = request
        .headers()
        .get("x-amz-content-sha256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("UNSIGNED-PAYLOAD")
        .to_string();

    // Build canonical request
    let uri = request.uri().clone();
    let method = request.method().clone();
    let path = uri.path();
    let query = uri.query().unwrap_or("");

    let canonical_request = build_canonical_request(
        &method,
        path,
        query,
        request.headers(),
        &auth_info.signed_headers,
        &payload_hash,
    );

    // Hash the canonical request
    let canonical_request_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

    // Build string to sign
    let credential_scope = format!("{}/{}/s3/aws4_request", auth_info.date, auth_info.region);

    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, credential_scope, canonical_request_hash
    );

    // Derive signing key and compute signature
    let signing_key = derive_signing_key(
        &config.secret_access_key,
        &auth_info.date,
        &auth_info.region,
    );

    let computed_signature = compute_signature(&signing_key, &string_to_sign);

    // Compare signatures
    if computed_signature != auth_info.signature {
        tracing::error!(
            "Signature mismatch! Computed: {}, Provided: {}",
            computed_signature,
            auth_info.signature
        );
        return Err(S3Error::SignatureDoesNotMatch(
            "The request signature we calculated does not match the signature you provided"
                .to_string(),
        ));
    }

    Ok(next.run(request).await)
}
