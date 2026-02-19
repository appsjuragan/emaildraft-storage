use anyhow::Result;
use async_trait::async_trait;

/// Trait defining operations for storing/retrieving chunks in email drafts.
/// Each provider (Gmail, Yahoo, etc.) implements this using IMAP.
#[async_trait]
pub trait EmailProvider: Send + Sync {
    /// Store data as an email draft attachment.
    /// Returns the IMAP UID of the created draft message.
    async fn create_draft(&self, subject: &str, attachment_data: &[u8]) -> Result<u32>;

    /// Retrieve the attachment data from a draft by its IMAP UID.
    async fn get_draft(&self, uid: u32) -> Result<Vec<u8>>;

    /// Delete a draft by its IMAP UID.
    async fn delete_draft(&self, uid: u32) -> Result<()>;

    /// Check connectivity / health
    async fn health_check(&self) -> Result<()>;
}
