use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "multipart_parts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub upload_id: Uuid,
    pub part_number: i32,
    pub size: i64,
    pub etag: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub temp_path: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::multipart_upload::Entity",
        from = "Column::UploadId",
        to = "super::multipart_upload::Column::Id"
    )]
    MultipartUpload,
}

impl Related<super::multipart_upload::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MultipartUpload.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
