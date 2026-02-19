use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(Migration001CreateTables)]
    }
}

pub struct Migration001CreateTables;

impl MigrationName for Migration001CreateTables {
    fn name(&self) -> &str {
        "m001_create_tables"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration001CreateTables {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // email_accounts table
        manager
            .create_table(
                Table::create()
                    .table(EmailAccounts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(EmailAccounts::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::Provider)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::Email)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::ImapHost)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::ImapPort)
                            .integer()
                            .not_null()
                            .default(993),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::PasswordEncrypted)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::DraftsFolder)
                            .string_len(255)
                            .not_null()
                            .default("[Gmail]/Drafts"),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::StorageUsed)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(EmailAccounts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // buckets table
        manager
            .create_table(
                Table::create()
                    .table(Buckets::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Buckets::Id).uuid().not_null().primary_key())
                    .col(
                        ColumnDef::new(Buckets::Name)
                            .string_len(63)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Buckets::OwnerId).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Buckets::Region)
                            .string_len(50)
                            .not_null()
                            .default("us-east-1"),
                    )
                    .col(
                        ColumnDef::new(Buckets::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // objects table
        manager
            .create_table(
                Table::create()
                    .table(Objects::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Objects::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Objects::BucketId).uuid().not_null())
                    .col(ColumnDef::new(Objects::Key).text().not_null())
                    .col(ColumnDef::new(Objects::Size).big_integer().not_null())
                    .col(ColumnDef::new(Objects::Etag).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Objects::ContentType)
                            .string_len(255)
                            .not_null()
                            .default("application/octet-stream"),
                    )
                    .col(ColumnDef::new(Objects::Metadata).json_binary().null())
                    .col(ColumnDef::new(Objects::ChunkCount).integer().not_null())
                    .col(
                        ColumnDef::new(Objects::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Objects::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Objects::Table, Objects::BucketId)
                            .to(Buckets::Table, Buckets::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique index on (bucket_id, key)
        manager
            .create_index(
                Index::create()
                    .name("idx_objects_bucket_key")
                    .table(Objects::Table)
                    .col(Objects::BucketId)
                    .col(Objects::Key)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // chunks table
        manager
            .create_table(
                Table::create()
                    .table(Chunks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Chunks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Chunks::ObjectId).uuid().not_null())
                    .col(ColumnDef::new(Chunks::ChunkIndex).integer().not_null())
                    .col(ColumnDef::new(Chunks::Size).big_integer().not_null())
                    .col(ColumnDef::new(Chunks::Hash).string_len(64).not_null())
                    .col(ColumnDef::new(Chunks::DraftUid).integer().not_null())
                    .col(ColumnDef::new(Chunks::EmailAccountId).uuid().not_null())
                    .col(
                        ColumnDef::new(Chunks::Status)
                            .string_len(20)
                            .not_null()
                            .default("active"),
                    )
                    .col(
                        ColumnDef::new(Chunks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Chunks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Chunks::Table, Chunks::ObjectId)
                            .to(Objects::Table, Objects::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Chunks::Table, Chunks::EmailAccountId)
                            .to(EmailAccounts::Table, EmailAccounts::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique index on (object_id, chunk_index)
        manager
            .create_index(
                Index::create()
                    .name("idx_chunks_object_index")
                    .table(Chunks::Table)
                    .col(Chunks::ObjectId)
                    .col(Chunks::ChunkIndex)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // multipart_uploads table
        manager
            .create_table(
                Table::create()
                    .table(MultipartUploads::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MultipartUploads::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MultipartUploads::BucketId).uuid().not_null())
                    .col(ColumnDef::new(MultipartUploads::Key).text().not_null())
                    .col(
                        ColumnDef::new(MultipartUploads::ContentType)
                            .string_len(255)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(MultipartUploads::Metadata)
                            .json_binary()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(MultipartUploads::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MultipartUploads::Table, MultipartUploads::BucketId)
                            .to(Buckets::Table, Buckets::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // multipart_parts table
        manager
            .create_table(
                Table::create()
                    .table(MultipartParts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(MultipartParts::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(MultipartParts::UploadId).uuid().not_null())
                    .col(
                        ColumnDef::new(MultipartParts::PartNumber)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipartParts::Size)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(MultipartParts::Etag)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(ColumnDef::new(MultipartParts::TempPath).text().null())
                    .col(
                        ColumnDef::new(MultipartParts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(MultipartParts::Table, MultipartParts::UploadId)
                            .to(MultipartUploads::Table, MultipartUploads::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique index on (upload_id, part_number)
        manager
            .create_index(
                Index::create()
                    .name("idx_multipart_parts_upload_part")
                    .table(MultipartParts::Table)
                    .col(MultipartParts::UploadId)
                    .col(MultipartParts::PartNumber)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(MultipartParts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(MultipartUploads::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Chunks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Objects::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Buckets::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(EmailAccounts::Table).to_owned())
            .await?;
        Ok(())
    }
}

// ========== Table identifiers ==========

#[derive(Iden)]
enum EmailAccounts {
    Table,
    Id,
    Provider,
    Email,
    ImapHost,
    ImapPort,
    PasswordEncrypted,
    DraftsFolder,
    StorageUsed,
    CreatedAt,
}

#[derive(Iden)]
enum Buckets {
    Table,
    Id,
    Name,
    OwnerId,
    Region,
    CreatedAt,
}

#[derive(Iden)]
enum Objects {
    Table,
    Id,
    BucketId,
    Key,
    Size,
    Etag,
    ContentType,
    Metadata,
    ChunkCount,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum Chunks {
    Table,
    Id,
    ObjectId,
    ChunkIndex,
    Size,
    Hash,
    DraftUid,
    EmailAccountId,
    Status,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
enum MultipartUploads {
    Table,
    Id,
    BucketId,
    Key,
    ContentType,
    Metadata,
    CreatedAt,
}

#[derive(Iden)]
enum MultipartParts {
    Table,
    Id,
    UploadId,
    PartNumber,
    Size,
    Etag,
    TempPath,
    CreatedAt,
}
