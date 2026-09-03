//! Database operations for milestones.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::milestone::{
    self, ActiveModel, Entity as MilestoneEntity, Model as Milestone,
};

/// Find a milestone by id.
pub async fn find_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Milestone>> {
    MilestoneEntity::find_by_id(id)
        .one(db)
        .await
        .context("db: find milestone by id")
}

/// List milestones for a repo.
pub async fn list_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    state: Option<&str>,
) -> Result<Vec<Milestone>> {
    let mut query = MilestoneEntity::find().filter(milestone::Column::RepoId.eq(repo_id));
    if let Some(s) = state {
        query = query.filter(milestone::Column::State.eq(s));
    }
    query
        .order_by_asc(milestone::Column::CreatedAt)
        .all(db)
        .await
        .context("db: list milestones by repo")
}

/// Create a new milestone.
pub async fn create(db: &DatabaseConnection, model: ActiveModel) -> Result<Milestone> {
    model.insert(db).await.context("db: create milestone")
}

/// Update a milestone.
pub async fn update(db: &DatabaseConnection, model: ActiveModel) -> Result<Milestone> {
    model.update(db).await.context("db: update milestone")
}

/// Delete a milestone by id.
pub async fn delete_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    MilestoneEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete milestone")?;
    Ok(())
}

/// Count open (non-closed) issues in a milestone.
pub async fn count_open_by_milestone(db: &DatabaseConnection, milestone_id: i64) -> Result<i64> {
    use crate::entities::issue;
    let count = issue::Entity::find()
        .filter(issue::Column::MilestoneId.eq(milestone_id))
        .filter(issue::Column::State.ne("closed"))
        .count(db)
        .await
        .context("db: count open issues by milestone")?;
    Ok(count as i64)
}

/// Batch-count (open, closed) issues for a set of milestones (A2 enrich).
///
/// Returns a map keyed by milestone id. Milestones without any matching
/// issues are present with `(0, 0)`.
pub async fn counts_by_milestones(
    db: &DatabaseConnection,
    milestone_ids: &[i64],
) -> Result<std::collections::HashMap<i64, (i64, i64)>> {
    use crate::entities::issue;
    use std::collections::HashMap;

    let mut out: HashMap<i64, (i64, i64)> = milestone_ids
        .iter()
        .map(|id| (*id, (0_i64, 0_i64)))
        .collect();
    if milestone_ids.is_empty() {
        return Ok(out);
    }

    let ids = milestone_ids.iter().copied();

    // open issues = state != closed
    let open: Vec<(Option<i64>, i64)> = issue::Entity::find()
        .select_only()
        .column(issue::Column::MilestoneId)
        .column_as(issue::Column::Id.count(), "cnt")
        .filter(issue::Column::MilestoneId.is_in(ids.clone()))
        .filter(issue::Column::State.ne("closed"))
        .group_by(issue::Column::MilestoneId)
        .into_tuple()
        .all(db)
        .await
        .context("db: count open issues by milestones")?;
    for (mid, cnt) in open {
        if let Some(mid) = mid {
            out.entry(mid).or_insert((0, 0)).0 = cnt;
        }
    }

    // closed issues = state == closed
    let closed: Vec<(Option<i64>, i64)> = issue::Entity::find()
        .select_only()
        .column(issue::Column::MilestoneId)
        .column_as(issue::Column::Id.count(), "cnt")
        .filter(issue::Column::MilestoneId.is_in(ids))
        .filter(issue::Column::State.eq("closed"))
        .group_by(issue::Column::MilestoneId)
        .into_tuple()
        .all(db)
        .await
        .context("db: count closed issues by milestones")?;
    for (mid, cnt) in closed {
        if let Some(mid) = mid {
            out.entry(mid).or_insert((0, 0)).1 = cnt;
        }
    }

    Ok(out)
}

/// Batch-fetch milestone titles by ids (A3 enrich for issue responses).
pub async fn titles_by_ids(
    db: &DatabaseConnection,
    milestone_ids: &[i64],
) -> Result<std::collections::HashMap<i64, String>> {
    use std::collections::HashMap;

    let mut out = HashMap::new();
    if milestone_ids.is_empty() {
        return Ok(out);
    }
    let rows = MilestoneEntity::find()
        .filter(milestone::Column::Id.is_in(milestone_ids.iter().copied()))
        .select_only()
        .column(milestone::Column::Id)
        .column(milestone::Column::Title)
        .into_tuple()
        .all(db)
        .await
        .context("db: fetch milestone titles")?;
    for (id, title) in rows {
        out.insert(id, title);
    }
    Ok(out)
}

/// Delete a milestone and detach every issue / pull request referencing it.
///
/// GitHub semantics: deleting a milestone clears the association on linked
/// issues rather than deleting them (A5). Runs in one transaction.
pub async fn delete_cascade(db: &DatabaseConnection, milestone_id: i64) -> Result<()> {
    use crate::entities::{issue, pull_request};
    use sea_orm::sea_query::Expr;
    use sea_orm::TransactionTrait;

    let txn = db
        .begin()
        .await
        .context("db: begin delete-milestone transaction")?;

    issue::Entity::update_many()
        .col_expr(issue::Column::MilestoneId, Expr::value(Option::<i64>::None))
        .filter(issue::Column::MilestoneId.eq(milestone_id))
        .exec(&txn)
        .await
        .context("db: detach issues from milestone")?;

    pull_request::Entity::update_many()
        .col_expr(
            pull_request::Column::MilestoneId,
            Expr::value(Option::<i64>::None),
        )
        .filter(pull_request::Column::MilestoneId.eq(milestone_id))
        .exec(&txn)
        .await
        .context("db: detach pull requests from milestone")?;

    MilestoneEntity::delete_by_id(milestone_id)
        .exec(&txn)
        .await
        .context("db: delete milestone")?;

    txn.commit()
        .await
        .context("db: commit delete-milestone transaction")?;
    Ok(())
}
