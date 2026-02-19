use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "buckets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub owner_id: String,
    pub region: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::object::Entity")]
    Objects,
    #[sea_orm(has_many = "super::multipart_upload::Entity")]
    MultipartUploads,
}

impl Related<super::object::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Objects.def()
    }
}

impl Related<super::multipart_upload::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MultipartUploads.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
