//! SeaORM entity for `pipeline_jobs` table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "pipeline_jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub stage_id: i64,
    pub name: String,
    pub image: Option<String>, // container image (future: Docker runner)
    pub script: String,        // shell commands (newline separated)
    pub status: String,        // pending, running, success, failed, skipped
    pub exit_code: Option<i32>,
    pub log: Option<String>,
    pub started_at: Option<DateTime>,
    pub finished_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pipeline_stage::Entity",
        from = "Column::StageId",
        to = "super::pipeline_stage::Column::Id"
    )]
    PipelineStage,
}

impl Related<super::pipeline_stage::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::PipelineStage.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
