use anyhow::{bail, Context, Result};
use async_imap::Session;
use async_native_tls::TlsStream;
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures::StreamExt;
use mail_builder::MessageBuilder;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use super::provider::EmailProvider;

use futures::io::{AsyncRead, AsyncWrite};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};

/// Wrapper for either TLS or Plain IMAP stream
enum StreamWrapper {
    Tls(TlsStream<Compat<TcpStream>>),
    Plain(Compat<TcpStream>),
}

impl AsyncRead for StreamWrapper {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            StreamWrapper::Tls(s) => Pin::new(s).poll_read(cx, buf),
            StreamWrapper::Plain(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for StreamWrapper {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            StreamWrapper::Tls(s) => Pin::new(s).poll_write(cx, buf),
            StreamWrapper::Plain(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamWrapper::Tls(s) => Pin::new(s).poll_flush(cx),
            StreamWrapper::Plain(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            StreamWrapper::Tls(s) => Pin::new(s).poll_close(cx),
            StreamWrapper::Plain(s) => Pin::new(s).poll_close(cx),
        }
    }
}

impl std::fmt::Debug for StreamWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamWrapper::Tls(_) => write!(f, "StreamWrapper::Tls"),
            StreamWrapper::Plain(_) => write!(f, "StreamWrapper::Plain"),
        }
    }
}

unsafe impl Send for StreamWrapper {}
impl Unpin for StreamWrapper {}

/// Gmail IMAP-based email provider.
/// Uses App Passwords for authentication (no OAuth2 needed).
pub struct GmailProvider {
    host: String,
    port: u16,
    email: String,
    password: String,
    drafts_folder: String,
    session: Mutex<Option<Session<StreamWrapper>>>,
}

impl GmailProvider {
    pub fn new(
        host: String,
        port: u16,
        email: String,
        password: String,
        drafts_folder: String,
    ) -> Self {
        Self {
            host,
            port,
            email,
            password,
            drafts_folder,
            session: Mutex::new(None),
        }
    }

    /// Get or create an IMAP session
    async fn get_session(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<Session<StreamWrapper>>>> {
        let mut guard = self.session.lock().await;
        if guard.is_none() {
            let session = self.connect().await?;
            *guard = Some(session);
        }
        Ok(guard)
    }

    /// Establish a new IMAP connection
    async fn connect(&self) -> Result<Session<StreamWrapper>> {
        tracing::info!("Connecting to IMAP {}:{}", self.host, self.port);

        let tcp = TcpStream::connect(format!("{}:{}", self.host, self.port))
            .await
            .context("Failed to connect to IMAP server")?;

        let stream = if self.port == 993 || self.port == 3993 {
            tracing::info!("Using IMAPS (TLS)");
            let tls = async_native_tls::TlsConnector::new();
            let tls_stream = tls
                .connect(&self.host, tcp.compat())
                .await
                .context("TLS handshake failed")?;
            StreamWrapper::Tls(tls_stream)
        } else {
            tracing::info!("Using plain IMAP");
            StreamWrapper::Plain(tcp.compat())
        };

        let client = async_imap::Client::new(stream);

        let mut session = client
            .login(&self.email, &self.password)
            .await
            .map_err(|(err, _)| err)
            .context("IMAP login failed")?;

        // Ensure drafts folder exists (ignore potential error if it already exists)
        let _ = session.create(&self.drafts_folder).await;

        tracing::info!("IMAP login successful for {}", self.email);
        Ok(session)
    }

    /// Reconnect if the session is stale
    async fn ensure_session(&self) -> Result<()> {
        let mut guard = self.session.lock().await;

        // Try a NOOP to see if connection is alive
        let needs_reconnect = if let Some(ref mut session) = *guard {
            session.noop().await.is_err()
        } else {
            true
        };

        if needs_reconnect {
            tracing::info!("Reconnecting IMAP session...");
            let session = self.connect().await?;
            *guard = Some(session);
        }

        Ok(())
    }

    /// Build an RFC 2822 MIME message with attachment
    fn build_mime_message(&self, subject: &str, attachment_data: &[u8]) -> Vec<u8> {
        MessageBuilder::new()
            .from(self.email.as_str())
            .to(self.email.as_str())
            .subject(subject)
            .text_body("ObjectMail chunk data")
            .attachment("application/octet-stream", "chunk.bin", attachment_data)
            .write_to_vec()
            .unwrap_or_default()
    }
}

#[async_trait]
impl EmailProvider for GmailProvider {
    async fn create_draft(&self, subject: &str, attachment_data: &[u8]) -> Result<u32> {
        self.ensure_session().await?;
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().context("No IMAP session")?;

        let mime_message = self.build_mime_message(subject, attachment_data);

        // APPEND to drafts folder with \Draft flag
        // In async-imap 0.10, append signature is:
        // append(folder, flags, internal_date, content)
        // Flags and date are Option<&str> (?) - let's try this based on error hints
        session
            .append(
                &self.drafts_folder,
                None::<&str>,
                None::<&str>,
                &mime_message,
            )
            .await
            .context("IMAP APPEND failed")?;

        // In 0.10, append doesn't seem to return the UID directly.
        // We must search for it.
        tracing::info!(
            "Draft appended to {}, searching for UID with subject: {}",
            self.drafts_folder,
            subject
        );

        // Fallback: search for the most recent message with our subject
        session
            .select(&self.drafts_folder)
            .await
            .context("Failed to SELECT drafts folder")?;

        // Search by subject - find messages with OBJMAIL: prefix
        let search_query = format!(
            "SUBJECT \"{}\"",
            &subject[..std::cmp::min(subject.len(), 100)]
        );
        let uids = session
            .uid_search(&search_query)
            .await
            .context("IMAP UID SEARCH failed")?;

        let uid = uids.into_iter().max().context(format!(
            "Could not find draft UID after APPEND with subject: {}",
            subject
        ))?;

        tracing::info!("Draft created, found UID via search: {}", uid);
        Ok(uid)
    }

    async fn get_draft(&self, uid: u32) -> Result<Vec<u8>> {
        self.ensure_session().await?;
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().context("No IMAP session")?;

        session
            .select(&self.drafts_folder)
            .await
            .context("Failed to SELECT drafts folder")?;

        // Fetch the full message by UID
        let mut fetch_stream = session
            .uid_fetch(uid.to_string(), "BODY[]")
            .await
            .context("IMAP UID FETCH failed")?;

        let mut raw_message: Option<Vec<u8>> = None;
        while let Some(result) = fetch_stream.next().await {
            let fetch = result.context("Error fetching message")?;
            if let Some(body) = fetch.body() {
                raw_message = Some(body.to_vec());
                break;
            }
        }
        drop(fetch_stream);

        let raw_message = raw_message.context("No message body found")?;

        // Parse MIME to extract attachment
        let parsed = mailparse::parse_mail(&raw_message).context("Failed to parse MIME message")?;

        // Find the attachment part (application/octet-stream)
        for part in &parsed.subparts {
            let content_type = part
                .headers
                .iter()
                .find(|h| h.get_key().to_lowercase() == "content-type")
                .map(|h| h.get_value())
                .unwrap_or_default();

            if content_type
                .to_lowercase()
                .contains("application/octet-stream")
            {
                let body = part
                    .get_body_raw()
                    .context("Failed to get attachment body")?;
                return Ok(body);
            }
        }

        // If no sub-parts, try attachment on main body
        if parsed.subparts.is_empty() {
            let body = parsed
                .get_body_raw()
                .context("Failed to get message body")?;
            return Ok(body);
        }

        bail!("No attachment found in draft UID {}", uid)
    }

    async fn delete_draft(&self, uid: u32) -> Result<()> {
        self.ensure_session().await?;
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().context("No IMAP session")?;

        session
            .select(&self.drafts_folder)
            .await
            .context("Failed to SELECT drafts folder")?;

        // Mark as deleted
        {
            let store_stream = session
                .uid_store(uid.to_string(), "+FLAGS (\\Deleted)")
                .await
                .context("IMAP UID STORE failed")?;
            tokio::pin!(store_stream);
            while let Some(_) = store_stream.next().await {}
        }

        // Expunge to permanently remove
        {
            let expunge_stream = session.expunge().await.context("IMAP EXPUNGE failed")?;
            tokio::pin!(expunge_stream);
            while let Some(_) = expunge_stream.next().await {}
        }

        tracing::info!("Draft UID {} deleted", uid);
        Ok(())
    }

    async fn health_check(&self) -> Result<()> {
        self.ensure_session().await?;
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().context("No IMAP session")?;

        session.noop().await.context("IMAP NOOP failed")?;
        Ok(())
    }
}
