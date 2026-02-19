use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub s3: S3Config,
    pub email: EmailConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    pub chunk_size_mb: u64,
    pub temp_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Config {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmailConfig {
    pub provider: String,
    pub address: String,
    pub password: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub drafts_folder: String,
}

impl AppConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        Ok(Self {
            server: ServerConfig {
                host: std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: std::env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "3000".to_string())
                    .parse()?,
            },
            database: DatabaseConfig {
                url: std::env::var("DATABASE_URL")?,
            },
            storage: StorageConfig {
                chunk_size_mb: std::env::var("STORAGE_CHUNK_SIZE_MB")
                    .unwrap_or_else(|_| "18".to_string())
                    .parse()?,
                temp_dir: PathBuf::from(
                    std::env::var("STORAGE_TEMP_DIR").unwrap_or_else(|_| "./tmp".to_string()),
                ),
            },
            s3: S3Config {
                access_key_id: std::env::var("S3_ACCESS_KEY_ID")
                    .unwrap_or_else(|_| "objectmail".to_string()),
                secret_access_key: std::env::var("S3_SECRET_ACCESS_KEY")
                    .unwrap_or_else(|_| "objectmail-secret-key".to_string()),
                region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
            },
            email: EmailConfig {
                provider: std::env::var("EMAIL_PROVIDER").unwrap_or_else(|_| "gmail".to_string()),
                address: std::env::var("EMAIL_ADDRESS")
                    .unwrap_or_else(|_| "user@gmail.com".to_string()),
                password: std::env::var("EMAIL_PASSWORD").unwrap_or_else(|_| String::new()),
                imap_host: std::env::var("EMAIL_IMAP_HOST")
                    .unwrap_or_else(|_| "imap.gmail.com".to_string()),
                imap_port: std::env::var("EMAIL_IMAP_PORT")
                    .unwrap_or_else(|_| "993".to_string())
                    .parse()?,
                drafts_folder: std::env::var("EMAIL_DRAFTS_FOLDER")
                    .unwrap_or_else(|_| "[Gmail]/Drafts".to_string()),
            },
        })
    }

    /// Returns chunk size in bytes
    pub fn chunk_size_bytes(&self) -> u64 {
        self.storage.chunk_size_mb * 1024 * 1024
    }
}
