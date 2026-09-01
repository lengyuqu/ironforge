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

// Repository row as returned by GET /repos/explore (enriched with owner_name).
export interface ExploreRepo {
  id: number;
  owner_id: number;
  owner_name: string;
  name: string;
  description: string | null;
  stars_count: number;
  forks_count: number;
  /** ISO 8601 */
  updated_at: string;
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
  repo_id?: number;
  number: number;
  title: string;
  body?: string | null;
  /** Backend stores free-form state strings ("open" / "closed"). */
  state: string;
  author_id?: number;
  /** Display username — enriched by IssueResponse; render with fallback. */
  author?: string | null;
  assignee?: string | null;
  /** Assignee usernames (primary first) — ISSUE-105. */
  assignees?: string[];
  milestone_id?: number | null;
  /** Normalised from the raw labels column by the api layer. */
  labels?: string[];
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
  /** open / merging (transient) / closed / merged */
  state: 'open' | 'merging' | 'closed' | 'merged';
  is_draft: boolean;
  author_id: number;
  /** Display username — present only when an endpoint enriches it; render with fallback */
  author?: string | null;
  head_branch: string;
  base_branch: string;
  head_sha?: string | null;
  /** Merge automatically once branch protection requirements are satisfied */
  auto_merge_enabled?: boolean;
  auto_merge_strategy?: string | null;
  merge_strategy?: string | null;
  created_at?: string;
  updated_at?: string;
  closed_at?: string | null;
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

// ── Repo overview (tree / log / repo info) ──────────────────────────

export interface RepoInfo {
  id: number;
  name: string;
  description: string | null;
  is_private: boolean;
  default_branch: string;
  stars_count: number;
  created_at: string;
}

export interface RepoTreeEntry {
  name: string;
  path?: string;
  kind: string; // "tree" | "blob"
  size?: number | null;
  sha?: string | null;
}

export interface RepoCommitEntry {
  sha: string;
  message: string;
  author: string;
  date: string;
}

export interface BlobContent {
  path: string;
  sha: string;
  size: number;
  content: string;
  encoding: string; // "utf-8" | "base64"
  is_binary: boolean;
  name?: string;
}

// ── Wiki ────────────────────────────────────────────────────────────────────

export interface WikiPage {
  id: number;
  title: string;
  content: string;
  message: string | null;
  author_id: number | null;
  /** ISO 8601 */
  created_at: string;
  /** ISO 8601 */
  updated_at: string;
}

export interface WikiPageSummary {
  id: number;
  title: string;
  /** ISO 8601 */
  updated_at: string;
}

export interface WikiRevision {
  id: number;
  wiki_page_id: number;
  content: string;
  message: string | null;
  author_id: number | null;
  version: number;
  /** ISO 8601 */
  created_at: string;
}

export interface Board {
  id: number;
  repo_id: number | null;
  org_id: number | null;
  name: string;
  description: string | null;
  created_by: number;
  /** ISO 8601 */
  created_at: string;
  /** ISO 8601 */
  updated_at: string;
}

export interface BoardColumn {
  id: number;
  board_id: number;
  name: string;
  color: string | null;
  position: number;
  /** ISO 8601 */
  created_at: string;
}

export interface BoardCard {
  /** rg-core CardFull: board_card::Model flatten + issue */
  id: number;
  column_id: number;
  issue_id: number | null;
  note: string | null;
  position: number;
  /** ISO 8601 */
  created_at: string;
  /** ISO 8601 */
  updated_at: string;
  /** Minimal issue metadata for issue-number links (rg-db issue::Model) */
  issue: { id: number; number: number; title: string } | null;
}

export interface BoardColumnFull {
  column: BoardColumn;
  cards: BoardCard[];
}

export interface BoardFull {
  board: Board;
  columns: BoardColumnFull[];
}

export interface Artifact {
  id: number;
  job_id: number;
  name: string;
  file_path: string;
  size: number;
  /** ISO 8601 */
  created_at: string;
  /** ISO 8601 */
  expires_at: string | null;
}

// ── Pipeline / CI ────────────────────────────────────────────────────────────

export interface Pipeline {
  id: number;
  repo_id: number;
  commit_sha: string;
  ref_name: string;
  status: string;
  trigger_type: string;
  triggered_by?: number | null;
  /** ISO 8601 or null */
  started_at?: string | null;
  /** ISO 8601 or null */
  finished_at?: string | null;
  /** ISO 8601 */
  created_at?: string;
}

export interface PipelineStage {
  id: number;
  pipeline_id: number;
  name: string;
  stage_order: number;
  status: string;
  /** ISO 8601 or null */
  started_at?: string | null;
  /** ISO 8601 or null */
  finished_at?: string | null;
}

export interface PipelineJob {
  id: number;
  stage_id: number;
  name: string;
  image?: string | null;
  script?: string;
  when_condition?: string;
  if_condition?: string | null;
  allow_failure?: boolean;
  timeout_seconds?: number | null;
  environment_id?: number | null;
  environment_name?: string | null;
  status: string;
  exit_code?: number | null;
  /** Only present in single-job responses */
  log?: string;
  /** ISO 8601 or null */
  started_at?: string | null;
  /** ISO 8601 or null */
  finished_at?: string | null;
}

export interface PipelineStageEntry {
  stage?: PipelineStage;
  jobs?: PipelineJob[];
}

/** GET /repos/:owner/:repo/pipelines/:id response */
export interface PipelineDetailResponse {
  pipeline: Pipeline;
  stages: PipelineStageEntry[];
}

/**
 * Flattened pipeline view used by the UI: pipeline fields plus `ref` (from
 * ref_name) and normalized `stages: Array<Stage & { jobs: PipelineJob[] }>`.
 */
export type PipelineDetail = Omit<Pipeline, 'ref_name'> & {
  ref: string;
  stages: Array<PipelineStage & { jobs: PipelineJob[] }>;
};

// ── Review / Comments ────────────────────────────────────────────────────────

export interface RequestedReviewer {
  id: number;
  reviewer_id: number;
  username: string;
  requested_by_id: number;
  created_at: string;
}

export interface PrTimelineEvent {
  id: string;
  kind: string;
  actor: { id: number; username: string } | null;
  created_at: string;
  body: string | null;
  metadata: Record<string, any>;
}

export interface PrReview {
  id: number;
  pr_id: number;
  repo_id: number;
  reviewer_id: number;
  /** comment / approve / request_changes / dismiss */
  action: string;
  body: string | null;
  commit_id: string | null;
  /** ISO 8601 */
  created_at: string;
}

export interface ReviewComment {
  id: number;
  body: string;
  /** Parent comment id for replies, null/undefined for root comments */
  reply_to_id?: number | null;
  path?: string;
  line?: number;
  start_line?: number;
  side?: 'LEFT' | 'RIGHT';
  /** Raw suggested content; empty string means "delete this range" */
  suggestion?: string | null;
  /** ISO 8601 or null */
  resolved_at?: string | null;
  commit_id?: string;
  /** ISO 8601 or null */
  suggestion_applied_at?: string | null;
}

export interface IssueComment {
  id: number;
  issue_id?: number;
  author_id?: number;
  body: string;
  /** Display username — enriched by CommentResponse; render with fallback. */
  author?: string | null;
  created_at?: string;
  updated_at?: string;
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
  repo_id?: number;
  tag_name: string;
  target_commitish: string;
  title: string;
  body?: string | null;
  is_draft: boolean;
  is_prerelease: boolean;
  author_id?: number;
  created_at?: string;
  updated_at?: string;
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
  repo_id?: number;
  name: string;
  color: string;
  description?: string | null;
  created_at?: string;
  updated_at?: string;
}

// ── Organization ────────────────────────────────────────────────────────────

export interface Organization {
  id: number;
  name: string;
  display_name: string | null;
  description: string | null;
  owner_id: number;
  visibility: string;
  created_at: string;
  updated_at: string;
}

export interface OrgSummary {
  id: number;
  name: string;
  display_name: string | null;
  visibility: string;
}

export interface OrganizationTeam {
  id: number;
  org_id: number;
  name: string;
  description: string | null;
  permission: string;
  created_at: string;
  updated_at: string;
}

export interface OrgMember {
  id: number;
  org_id: number;
  user_id: number;
  role: string;
  created_at: string;
}

export interface TeamMember {
  id: number;
  team_id: number;
  user_id: number;
  role: string;
  created_at: string;
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
  id: number;
  repo_id?: number;
  sha: string;
  state: string; // success | failure | error | pending
  context: string;
  description?: string | null;
  target_url?: string | null;
  creator_id?: number;
  created_at?: string;
  updated_at?: string;
}

export interface CombinedCommitStatus {
  state: string;
  sha: string;
  total_count: number;
  statuses: CommitStatus[];
}

// ── Time Tracking ───────────────────────────────────────────────────────────

export interface TimeEntry {
  id: number;
  issue_id: number;
  seconds: number;
  created_at?: string;
}
