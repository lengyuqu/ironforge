//! Reaction entity — maps to the `reactions` table.
//!
//! `comment_id = 0` targets the issue itself; non-zero targets an issue
//! comment (same convention as Gitea).

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "reactions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Issue this reaction belongs to (issue body or one of its comments)
    pub issue_id: i64,
    /// 0 = reaction on the issue itself; non-zero targets an issue comment
    pub comment_id: i64,
    /// User who reacted
    pub user_id: i64,
    /// Reaction content: `+1`, `-1`, `laugh`, `confused`, `heart`, `hooray`, `rocket`, `eyes`
    pub content: String,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::issue::Entity",
        from = "Column::IssueId",
        to = "super::issue::Column::Id"
    )]
    Issue,
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl Related<super::issue::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Issue.def()
    }
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
