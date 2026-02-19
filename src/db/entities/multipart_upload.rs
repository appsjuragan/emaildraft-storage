use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "multipart_uploads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub bucket_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub key: String,
    pub content_type: Option<String>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::bucket::Entity",
        from = "Column::BucketId",
        to = "super::bucket::Column::Id"
    )]
    Bucket,
    #[sea_orm(has_many = "super::multipart_part::Entity")]
    Parts,
}

impl Related<super::bucket::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bucket.def()
    }
}

impl Related<super::multipart_part::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Parts.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
