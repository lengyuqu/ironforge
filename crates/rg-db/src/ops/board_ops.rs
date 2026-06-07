//! Database operations for project boards, columns, and cards.

use anyhow::{Context, Result};
use sea_orm::*;

use crate::entities::board::{self, ActiveModel as BoardAM, Entity as BoardEntity, Model as Board};
use crate::entities::board_card::{
    self, ActiveModel as CardAM, Entity as CardEntity, Model as Card,
};
use crate::entities::board_column::{
    self, ActiveModel as ColumnAM, Entity as ColumnEntity, Model as Column,
};

// ── Board ────────────────────────────────────────────────────────────────

pub async fn create_board(db: &DatabaseConnection, model: BoardAM) -> Result<Board> {
    model.insert(db).await.context("db: create board")
}

pub async fn find_board_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Board>> {
    BoardEntity::find_by_id(id).one(db).await.context("db: find board")
}

pub async fn list_boards_by_repo(
    db: &DatabaseConnection,
    repo_id: i64,
) -> Result<Vec<Board>> {
    BoardEntity::find()
        .filter(board::Column::RepoId.eq(repo_id))
        .order_by_asc(board::Column::Name)
        .all(db)
        .await
        .context("db: list boards by repo")
}

pub async fn list_boards_by_org(
    db: &DatabaseConnection,
    org_id: i64,
) -> Result<Vec<Board>> {
    BoardEntity::find()
        .filter(board::Column::OrgId.eq(org_id))
        .order_by_asc(board::Column::Name)
        .all(db)
        .await
        .context("db: list boards by org")
}

pub async fn update_board(db: &DatabaseConnection, model: BoardAM) -> Result<Board> {
    model.update(db).await.context("db: update board")
}

pub async fn delete_board_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    BoardEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete board")?;
    Ok(())
}

// ── Column ───────────────────────────────────────────────────────────────

pub async fn create_column(db: &DatabaseConnection, model: ColumnAM) -> Result<Column> {
    model.insert(db).await.context("db: create column")
}

pub async fn find_column_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Column>> {
    ColumnEntity::find_by_id(id).one(db).await.context("db: find column")
}

pub async fn list_columns_by_board(
    db: &DatabaseConnection,
    board_id: i64,
) -> Result<Vec<Column>> {
    ColumnEntity::find()
        .filter(board_column::Column::BoardId.eq(board_id))
        .order_by_asc(board_column::Column::Position)
        .all(db)
        .await
        .context("db: list columns by board")
}

pub async fn update_column(db: &DatabaseConnection, model: ColumnAM) -> Result<Column> {
    model.update(db).await.context("db: update column")
}

pub async fn delete_column_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    ColumnEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete column")?;
    Ok(())
}

// ── Card ─────────────────────────────────────────────────────────────────

pub async fn create_card(db: &DatabaseConnection, model: CardAM) -> Result<Card> {
    model.insert(db).await.context("db: create card")
}

pub async fn find_card_by_id(db: &DatabaseConnection, id: i64) -> Result<Option<Card>> {
    CardEntity::find_by_id(id).one(db).await.context("db: find card")
}

pub async fn list_cards_by_column(
    db: &DatabaseConnection,
    column_id: i64,
) -> Result<Vec<Card>> {
    CardEntity::find()
        .filter(board_card::Column::ColumnId.eq(column_id))
        .order_by_asc(board_card::Column::Position)
        .all(db)
        .await
        .context("db: list cards by column")
}

pub async fn update_card(db: &DatabaseConnection, model: CardAM) -> Result<Card> {
    model.update(db).await.context("db: update card")
}

pub async fn delete_card_by_id(db: &DatabaseConnection, id: i64) -> Result<()> {
    CardEntity::delete_by_id(id)
        .exec(db)
        .await
        .context("db: delete card")?;
    Ok(())
}

/// Batch update card positions (for reorder).
pub async fn update_card_positions(
    db: &DatabaseConnection,
    positions: &[(i64, i32)],
) -> Result<()> {
    for (card_id, pos) in positions {
        CardEntity::update_many()
            .col_expr(board_card::Column::Position, sea_query::Expr::value(*pos))
            .filter(board_card::Column::Id.eq(*card_id))
            .exec(db)
            .await
            .context("db: update card position")?;
    }
    Ok(())
}
