//! Board service — business logic for project boards (Kanban).
//!
//! Boards belong to a repository or organization. Each board has
//! columns, and each column has cards. Cards can be linked to
//! issues or be free-text notes.

use crate::error::{CoreContext, CoreError, CoreResult};
use chrono::Utc;
use rg_db::entities::board::{ActiveModel as BoardAM, Model as Board};
use rg_db::entities::board_card::{ActiveModel as CardAM, Model as Card};
use rg_db::entities::board_column::{ActiveModel as ColumnAM, Model as Column};
use rg_db::entities::issue::Model as Issue;
use sea_orm::{ActiveValue::Set, DatabaseConnection, TransactionTrait};

// ── Scope checks ─────────────────────────────────────────────────────────
//
// Board, column and card ids arrive in URL paths next to the repository the
// caller was authorised against. Every lookup below is therefore anchored to
// that repository (and board), so an id that exists in some other repository
// is reported as not found instead of being read or modified.

async fn require_board(db: &DatabaseConnection, repo_id: i64, board_id: i64) -> CoreResult<Board> {
    match rg_db::ops::board_ops::find_board_by_id(db, board_id).await? {
        Some(board) if board.repo_id == Some(repo_id) => Ok(board),
        _ => Err(CoreError::not_found("board not found")),
    }
}

async fn require_column(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    column_id: i64,
) -> CoreResult<Column> {
    require_board(db, repo_id, board_id).await?;
    match rg_db::ops::board_ops::find_column_by_id(db, column_id).await? {
        Some(column) if column.board_id == board_id => Ok(column),
        _ => Err(CoreError::not_found("column not found")),
    }
}

async fn require_card(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    card_id: i64,
) -> CoreResult<Card> {
    require_board(db, repo_id, board_id).await?;
    let card = rg_db::ops::board_ops::find_card_by_id(db, card_id)
        .await?
        .ok_or_else(|| CoreError::not_found("card not found"))?;
    match rg_db::ops::board_ops::find_column_by_id(db, card.column_id).await? {
        Some(column) if column.board_id == board_id => Ok(card),
        _ => Err(CoreError::not_found("card not found")),
    }
}

/// A card may only link to an issue of the board's own repository; otherwise
/// the board view would expose another repository's issue.
async fn ensure_issue_in_repo(
    db: &DatabaseConnection,
    repo_id: i64,
    issue_id: i64,
) -> CoreResult<()> {
    match rg_db::ops::issue_ops::find_by_id(db, issue_id).await? {
        Some(issue) if issue.repo_id == repo_id => Ok(()),
        _ => Err(CoreError::invalid_input(format!(
            "issue {issue_id} not found in this repository"
        ))),
    }
}

fn require_name(value: &str, what: &str) -> CoreResult<()> {
    if value.trim().is_empty() {
        return Err(CoreError::invalid_input(format!("{what} cannot be empty")));
    }
    Ok(())
}

// ── Board CRUD ───────────────────────────────────────────────────────────

/// Create a new project board.
///
/// The board row and its default columns are written in one transaction —
/// a board left without its columns is unusable.
pub async fn create_board(
    db: &DatabaseConnection,
    name: String,
    description: Option<String>,
    repo_id: Option<i64>,
    org_id: Option<i64>,
    created_by: i64,
) -> CoreResult<Board> {
    require_name(&name, "board name")?;
    let now = Utc::now();
    let model = BoardAM {
        name: Set(name),
        description: Set(description),
        repo_id: Set(repo_id),
        org_id: Set(org_id),
        created_by: Set(created_by),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    let txn = db
        .begin()
        .await
        .context("db: begin create-board transaction")?;
    let board = rg_db::ops::board_ops::create_board(&txn, model).await?;

    // Auto-create default columns
    for (i, (name, color)) in ["To Do", "In Progress", "Done"]
        .iter()
        .zip(["#6366f1", "#f59e0b", "#22c55e"])
        .enumerate()
    {
        let col = ColumnAM {
            board_id: Set(board.id),
            name: Set(name.to_string()),
            color: Set(Some(color.to_string())),
            position: Set(i as i32),
            created_at: Set(now),
            ..Default::default()
        };
        rg_db::ops::board_ops::create_column(&txn, col).await?;
    }
    txn.commit()
        .await
        .context("db: commit create-board transaction")?;

    Ok(board)
}

/// Get a board of the given repository with all columns and cards.
pub async fn get_board(db: &DatabaseConnection, repo_id: i64, id: i64) -> CoreResult<BoardFull> {
    let board = require_board(db, repo_id, id).await?;

    let columns = rg_db::ops::board_ops::list_columns_by_board(db, board.id).await?;
    let mut columns_full = Vec::new();

    for col in columns {
        let cards = rg_db::ops::board_ops::list_cards_by_column(db, col.id).await?;
        let mut cards_full = Vec::with_capacity(cards.len());
        for card in cards {
            let issue = match card.issue_id {
                Some(issue_id) => rg_db::ops::issue_ops::find_by_id(db, issue_id).await?,
                None => None,
            };
            cards_full.push(CardFull { card, issue });
        }
        columns_full.push(ColumnFull {
            column: col,
            cards: cards_full,
        });
    }

    Ok(BoardFull {
        board,
        columns: columns_full,
    })
}

/// List boards for a repository.
pub async fn list_boards_by_repo(db: &DatabaseConnection, repo_id: i64) -> CoreResult<Vec<Board>> {
    Ok(rg_db::ops::board_ops::list_boards_by_repo(db, repo_id).await?)
}

/// List boards for an organization.
pub async fn list_boards_by_org(db: &DatabaseConnection, org_id: i64) -> CoreResult<Vec<Board>> {
    Ok(rg_db::ops::board_ops::list_boards_by_org(db, org_id).await?)
}

/// Update a board's metadata.
pub async fn update_board(
    db: &DatabaseConnection,
    repo_id: i64,
    id: i64,
    name: Option<String>,
    description: Option<String>,
) -> CoreResult<Board> {
    let existing = require_board(db, repo_id, id).await?;

    let mut model: BoardAM = existing.into();
    if let Some(v) = name {
        require_name(&v, "board name")?;
        model.name = Set(v);
    }
    if let Some(v) = description {
        model.description = Set(Some(v));
    }
    model.updated_at = Set(Utc::now());

    Ok(rg_db::ops::board_ops::update_board(db, model).await?)
}

/// Delete a board of the given repository.
pub async fn delete_board(db: &DatabaseConnection, repo_id: i64, id: i64) -> CoreResult<()> {
    require_board(db, repo_id, id).await?;
    Ok(rg_db::ops::board_ops::delete_board_by_id(db, id).await?)
}

// ── Column CRUD ──────────────────────────────────────────────────────────

/// Create a new column.
pub async fn create_column(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    name: String,
    color: Option<String>,
) -> CoreResult<Column> {
    require_board(db, repo_id, board_id).await?;
    require_name(&name, "column name")?;

    // Get the next position
    let columns = rg_db::ops::board_ops::list_columns_by_board(db, board_id).await?;
    let pos = columns.len() as i32;

    let now = Utc::now();
    let model = ColumnAM {
        board_id: Set(board_id),
        name: Set(name),
        color: Set(color),
        position: Set(pos),
        created_at: Set(now),
        ..Default::default()
    };

    Ok(rg_db::ops::board_ops::create_column(db, model).await?)
}

/// Update a column.
pub async fn update_column(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    id: i64,
    name: Option<String>,
    color: Option<String>,
) -> CoreResult<Column> {
    let existing = require_column(db, repo_id, board_id, id).await?;

    let mut model: ColumnAM = existing.into();
    if let Some(v) = name {
        require_name(&v, "column name")?;
        model.name = Set(v);
    }
    if let Some(v) = color {
        model.color = Set(Some(v));
    }
    Ok(rg_db::ops::board_ops::update_column(db, model).await?)
}

/// Delete a column.
pub async fn delete_column(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    id: i64,
) -> CoreResult<()> {
    require_column(db, repo_id, board_id, id).await?;
    Ok(rg_db::ops::board_ops::delete_column_by_id(db, id).await?)
}

// ── Card CRUD ────────────────────────────────────────────────────────────

/// Create a new card.
pub async fn create_card(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    column_id: i64,
    issue_id: Option<i64>,
    note: Option<String>,
) -> CoreResult<Card> {
    require_column(db, repo_id, board_id, column_id).await?;
    if let Some(issue_id) = issue_id {
        ensure_issue_in_repo(db, repo_id, issue_id).await?;
    }

    let cards = rg_db::ops::board_ops::list_cards_by_column(db, column_id).await?;
    let pos = cards.len() as i32;
    let now = Utc::now();

    let model = CardAM {
        column_id: Set(column_id),
        issue_id: Set(issue_id),
        note: Set(note),
        position: Set(pos),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    Ok(rg_db::ops::board_ops::create_card(db, model).await?)
}

/// Update a card's note or issue link.
pub async fn update_card(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    id: i64,
    note: Option<String>,
    issue_id: Option<Option<i64>>,
) -> CoreResult<Card> {
    let existing = require_card(db, repo_id, board_id, id).await?;
    if let Some(Some(issue_id)) = issue_id {
        ensure_issue_in_repo(db, repo_id, issue_id).await?;
    }

    let mut model: CardAM = existing.into();
    if let Some(v) = note {
        model.note = Set(Some(v));
    }
    if let Some(v) = issue_id {
        model.issue_id = Set(v);
    }
    model.updated_at = Set(Utc::now());

    Ok(rg_db::ops::board_ops::update_card(db, model).await?)
}

/// Move a card to another column of the same board with a specific position.
pub async fn move_card(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    card_id: i64,
    new_column_id: i64,
    new_position: i32,
) -> CoreResult<Card> {
    let existing = require_card(db, repo_id, board_id, card_id).await?;
    // The target column comes from the request body, so a bad reference is a
    // client error (400) rather than a missing URL resource.
    match rg_db::ops::board_ops::find_column_by_id(db, new_column_id).await? {
        Some(column) if column.board_id == board_id => {}
        _ => {
            return Err(CoreError::invalid_input(format!(
                "column {new_column_id} not found on this board"
            )))
        }
    }

    let mut model: CardAM = existing.into();
    model.column_id = Set(new_column_id);
    model.position = Set(new_position);
    model.updated_at = Set(Utc::now());

    Ok(rg_db::ops::board_ops::update_card(db, model).await?)
}

/// Reorder cards of a board.
pub async fn reorder_cards(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    positions: Vec<(i64, i32)>,
) -> CoreResult<()> {
    require_board(db, repo_id, board_id).await?;

    // Every referenced card must live on this board.
    let mut board_cards = std::collections::HashSet::new();
    for column in rg_db::ops::board_ops::list_columns_by_board(db, board_id).await? {
        for card in rg_db::ops::board_ops::list_cards_by_column(db, column.id).await? {
            board_cards.insert(card.id);
        }
    }
    if let Some((card_id, _)) = positions.iter().find(|(id, _)| !board_cards.contains(id)) {
        return Err(CoreError::invalid_input(format!(
            "card {card_id} not found on this board"
        )));
    }

    Ok(rg_db::ops::board_ops::update_card_positions(db, &positions).await?)
}

/// Delete a card.
pub async fn delete_card(
    db: &DatabaseConnection,
    repo_id: i64,
    board_id: i64,
    id: i64,
) -> CoreResult<()> {
    require_card(db, repo_id, board_id, id).await?;
    Ok(rg_db::ops::board_ops::delete_card_by_id(db, id).await?)
}

// ── Full response types ──────────────────────────────────────────────────

/// A full board view with columns and cards.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BoardFull {
    pub board: Board,
    pub columns: Vec<ColumnFull>,
}

/// A column with its cards.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ColumnFull {
    pub column: Column,
    pub cards: Vec<CardFull>,
}

/// A card plus optional issue metadata for frontend issue-number links.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct CardFull {
    #[serde(flatten)]
    pub card: Card,
    pub issue: Option<Issue>,
}
