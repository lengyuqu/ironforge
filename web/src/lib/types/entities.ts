// Shared domain entities — used across multiple API modules and stores.
//
// These are the canonical shapes.  API modules re-export from here when they
// return a type that matches, rather than re-declaring their own version.
// Fields align with the backend's utoipa annotations; the backend remains
// the single source of truth.

export interface User {
  id: number;
  username: string;
  email: string;
  display_name: string | null;
  is_admin: boolean;
  /** ISO 8601 */
  created_at?: string;
}

export interface Repo {
  id: number;
  name: string;
  full_name?: string; // "owner/name" shortcut, sometimes omitted in list views
  description: string | null;
  is_private: boolean;
  is_mirror?: boolean;
  stars_count: number;
  forks_count: number;
  watchers_count?: number;
  open_issues_count?: number;
  default_branch?: string;
  language?: string | null;
  owner: { id: number; username: string; avatar_url?: string | null };
  created_at?: string;
  updated_at?: string;
}

export interface Issue {
  id: number;
  number: number;
  title: string;
  body: string | null;
  state: 'open' | 'closed';
  author: { id: number; username: string };
  labels?: { id: number; name: string; color: string | null }[];
  milestone?: { id: number; title: string } | null;
  assignees?: { id: number; username: string }[];
  comments_count?: number;
  created_at?: string;
  updated_at?: string;
  closed_at?: string | null;
}

export interface PullRequest {
  id: number;
  number: number;
  title: string;
  body: string | null;
  state: 'open' | 'closed' | 'merged';
  draft?: boolean;
  author: { id: number; username: string };
  head?: { ref: string; sha: string };
  base?: { ref: string; sha: string };
  labels?: { id: number; name: string; color: string | null }[];
  comments_count?: number;
  review_comments_count?: number;
  created_at?: string;
  updated_at?: string;
  merged_at?: string | null;
}

export interface Commit {
  sha: string;
  message: string;
  author: { name: string; email: string; date?: string };
  committer: { name: string; email: string; date?: string };
}

// ── Branch / Tree ────────────────────────────────────────────────────────────

export interface Branch {
  name: string;
  is_default?: boolean;
  commit?: { sha: string };
}

export interface TreeEntry {
  name: string;
  type: 'blob' | 'tree' | 'commit';
  sha: string;
  size?: number;
}

// ── Wiki ────────────────────────────────────────────────────────────────────

export interface WikiPage {
  id?: number;
  title: string;
  slug?: string;
  content?: string;
  /** ISO 8601 */
  updated_at?: string;
}

export interface WikiRevision {
  id: number;
  title?: string;
  message?: string;
  author?: { username: string };
  /** ISO 8601 */
  created_at: string;
}

// ── Board / Kanban ──────────────────────────────────────────────────────────

export interface Board {
  id: number;
  name: string;
  description?: string;
  columns?: BoardColumnEntry[];
  /** ISO 8601 */
  created_at?: string;
}

export interface BoardColumn {
  id: number;
  name: string;
}

export interface BoardCard {
  id: number;
  note?: string;
  issue_id?: number;
  column_id?: number;
}

/** API returns either { column, cards } wrapper or flat column + inline cards */
export type BoardColumnEntry =
  | { column: BoardColumn; cards: BoardCard[] }
  | (BoardColumn & { cards?: BoardCard[] });

// ── Pipeline / CI ────────────────────────────────────────────────────────────

export interface PipelineStage {
  id?: number;
  name: string;
}

export interface PipelineJob {
  id: number;
  name: string;
  status: string;
  step?: number;
}

export interface PipelineStageEntry {
  stage?: PipelineStage;
  jobs?: PipelineJob[];
}

export interface Pipeline {
  id: number;
  ref_name: string;
  status: string;
  stages?: PipelineStageEntry[];
  /** ISO 8601 */
  created_at?: string;
}

// ── Review / Comments ────────────────────────────────────────────────────────

export interface ReviewComment {
  id: number;
  body: string;
  /** ISO 8601 or null */
  resolved_at?: string | null;
  commit_id?: string;
  /** ISO 8601 or null */
  suggestion_applied_at?: string | null;
}

export interface IssueComment {
  id: number;
  body: string;
  author: { id: number; username: string };
}

// ── Release / Asset ─────────────────────────────────────────────────────────

export interface ReleaseAsset {
  id: number;
  name: string;
  size: number;
  download_count?: number;
  /** ISO 8601 */
  created_at?: string;
}

// ── Runner ──────────────────────────────────────────────────────────────────

export interface Runner {
  id: number;
  name: string;
  status: string;
  labels: string[];
  version: string | null;
  os: string | null;
  arch: string | null;
  /** ISO 8601 */
  last_seen_at?: string;
}

// ── Package Registry ─────────────────────────────────────────────────────────

export interface PackageSummary {
  id: number;
  name: string;
  description: string | null;
  latest_version: string | null;
  download_count: number;
  format?: string;
}

// ── Release ─────────────────────────────────────────────────────────────────

export interface Release {
  id: number;
  tag_name: string;
  title: string;
  body: string | null;
  is_draft?: boolean;
  is_prerelease?: boolean;
  target_commitish?: string;
  author?: { id: number; username: string };
  assets_count?: number;
  /** ISO 8601 */
  created_at?: string;
  published_at?: string | null;
}

// ── Merge / PR ──────────────────────────────────────────────────────────────

export interface MergeResult {
  sha?: string;
  merged_at?: string;
  message?: string;
}

export interface MergeQueueEntry {
  number: number;
  title: string;
  strategy: string;
  author?: { username: string };
  /** ISO 8601 */
  created_at?: string;
}

// ── Milestone ───────────────────────────────────────────────────────────────

export interface Milestone {
  id: number;
  title: string;
  description?: string | null;
  state?: 'open' | 'closed';
  open_issues?: number;
  closed_issues?: number;
  due_on?: string | null;
}

// ── Label ───────────────────────────────────────────────────────────────────

export interface Label {
  id: number;
  name: string;
  color: string | null;
  description?: string | null;
}

// ── Organization ────────────────────────────────────────────────────────────

export interface Organization {
  id: number;
  name: string;
  display_name?: string;
  description?: string | null;
  avatar_url?: string | null;
}

export interface OrganizationTeam {
  id: number;
  name: string;
  description?: string | null;
}

// ── Notification ────────────────────────────────────────────────────────────

export interface Notification {
  id: number;
  type: string;
  title?: string;
  is_read?: boolean;
  created_at?: string;
  reason?: string;
  url?: string;
}

export interface UnreadCount {
  count: number;
}

// ── Collaborator ─────────────────────────────────────────────────────────────

export interface Collaborator {
  id: number;
  username: string;
  permission: 'read' | 'write' | 'admin';
  avatar_url?: string | null;
}

// ── Commit Status ───────────────────────────────────────────────────────────

export interface CommitStatus {
  context: string;
  state: 'pending' | 'success' | 'failure' | 'error';
  description?: string;
  target_url?: string;
}

// ── Time Tracking ───────────────────────────────────────────────────────────

export interface TimeEntry {
  id: number;
  issue_id: number;
  seconds: number;
  created_at?: string;
}
