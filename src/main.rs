mod config;
mod db;
mod email;
mod migration;
mod s3;
mod storage;

use std::sync::Arc;
use tokio::sync::Mutex;

use config::AppConfig;
use email::gmail::GmailProvider;
use email::provider::EmailProvider;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};
use sea_orm_migration::MigratorTrait;
use storage::pipeline::StoragePipeline;
use uuid::Uuid;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub db: DatabaseConnection,
    pub pipeline: Arc<Mutex<StoragePipeline>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
        )
        .init();

    // Load configuration
    let config = AppConfig::from_env()?;
    tracing::info!("ObjectMail starting...");
    tracing::info!("Server: {}:{}", config.server.host, config.server.port);
    tracing::info!(
        "Email: {} ({})",
        config.email.address,
        config.email.provider
    );
    tracing::info!("Chunk size: {} MB", config.storage.chunk_size_mb);

    // Create temp directory
    tokio::fs::create_dir_all(&config.storage.temp_dir).await?;

    // Connect to database
    let db = db::connect(&config.database.url).await?;

    // Run migrations
    migration::Migrator::up(&db, None).await?;
    tracing::info!("Database migrations complete");

    // Get or create email account record
    let email_account_id = ensure_email_account(&db, &config).await?;

    // Initialize email provider
    let email_provider: Arc<dyn EmailProvider> = Arc::new(GmailProvider::new(
        config.email.imap_host.clone(),
        config.email.imap_port,
        config.email.address.clone(),
        config.email.password.clone(),
        config.email.drafts_folder.clone(),
    ));

    // Initialize storage pipeline
    let pipeline =
        StoragePipeline::new(config.clone(), db.clone(), email_provider, email_account_id);

    // Build app state
    let state = AppState {
        config: config.clone(),
        db,
        pipeline: Arc::new(Mutex::new(pipeline)),
    };

    // Build router
    let app = s3::router::build_router(state);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("ObjectMail S3 API listening on {}", addr);
    tracing::info!(
        "Use with aws-cli: aws --endpoint-url http://localhost:{} s3 ...",
        config.server.port
    );

    axum::serve(listener, app).await?;

    Ok(())
}

/// Ensure an email account record exists in the database
async fn ensure_email_account(db: &DatabaseConnection, config: &AppConfig) -> anyhow::Result<Uuid> {
    use crate::db::entities::email_account;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    // Check if account already exists
    let existing = email_account::Entity::find()
        .filter(email_account::Column::Email.eq(&config.email.address))
        .one(db)
        .await?;

    if let Some(account) = existing {
        return Ok(account.id);
    }

    // Create new account
    let id = Uuid::new_v4();
    let account = email_account::ActiveModel {
        id: Set(id),
        provider: Set(config.email.provider.clone()),
        email: Set(config.email.address.clone()),
        imap_host: Set(config.email.imap_host.clone()),
        imap_port: Set(config.email.imap_port as i32),
        password_encrypted: Set(config.email.password.clone()), // TODO: encrypt at rest
        drafts_folder: Set(config.email.drafts_folder.clone()),
        storage_used: Set(0),
        created_at: Set(chrono::Utc::now()),
    };

    account.insert(db).await?;
    tracing::info!("Email account '{}' registered", config.email.address);

    Ok(id)
}
