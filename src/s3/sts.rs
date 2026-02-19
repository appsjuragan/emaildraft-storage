use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;

use crate::AppState;

/// Handle STS AssumeRole requests (POST /)
/// The MinIO Console uses STS to get temporary credentials before using the S3 API.
/// We respond with the same static credentials from config, acting as a pass-through STS.
pub async fn assume_role(
    State(state): State<AppState>,
    body: String,
) -> Response {
    // Parse form body to get Action
    let params: HashMap<&str, &str> = body
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next()?;
            let v = parts.next().unwrap_or("");
            Some((k, v))
        })
        .collect();

    let action = params.get("Action").copied().unwrap_or("");

    tracing::info!("STS request: Action={}", action);

    // For any STS action (AssumeRole, GetSessionToken, etc.), return static credentials
    let access_key = &state.config.s3.access_key_id;
    let secret_key = &state.config.s3.secret_access_key;
    let expiry = "2099-01-01T00:00:00Z";

    let xml = match action {
        "AssumeRole" => format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <Credentials>
      <AccessKeyId>{access_key}</AccessKeyId>
      <SecretAccessKey>{secret_key}</SecretAccessKey>
      <SessionToken>objectmail-session-token</SessionToken>
      <Expiration>{expiry}</Expiration>
    </Credentials>
    <AssumedRoleUser>
      <Arn>arn:aws:iam::000000000000:assumed-role/objectmail/objectmail</Arn>
      <AssumedRoleId>objectmail</AssumedRoleId>
    </AssumedRoleUser>
  </AssumeRoleResult>
  <ResponseMetadata>
    <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
  </ResponseMetadata>
</AssumeRoleResponse>"#,
            access_key = access_key,
            secret_key = secret_key,
            expiry = expiry
        ),
        "GetSessionToken" => format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetSessionTokenResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetSessionTokenResult>
    <Credentials>
      <AccessKeyId>{access_key}</AccessKeyId>
      <SecretAccessKey>{secret_key}</SecretAccessKey>
      <SessionToken>objectmail-session-token</SessionToken>
      <Expiration>{expiry}</Expiration>
    </Credentials>
  </GetSessionTokenResult>
  <ResponseMetadata>
    <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
  </ResponseMetadata>
</GetSessionTokenResponse>"#,
            access_key = access_key,
            secret_key = secret_key,
            expiry = expiry
        ),
        _ => format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <Credentials>
      <AccessKeyId>{access_key}</AccessKeyId>
      <SecretAccessKey>{secret_key}</SecretAccessKey>
      <SessionToken>objectmail-session-token</SessionToken>
      <Expiration>{expiry}</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata>
    <RequestId>00000000-0000-0000-0000-000000000000</RequestId>
  </ResponseMetadata>
</AssumeRoleResponse>"#,
            access_key = access_key,
            secret_key = secret_key,
            expiry = expiry
        ),
    };

    (
        StatusCode::OK,
        [("Content-Type", "text/xml")],
        xml,
    )
        .into_response()
}
