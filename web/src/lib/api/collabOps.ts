// Collaboration "ops" client: task templates, search, repository rules, escalation
// settings, session capability declarations and the deletion-impact preview.
//
// Only endpoints a signed-in person can call WITHOUT session credentials are
// exposed. `POST …/tasks/from-template` is deliberately NOT here: the server
// authenticates it as a session (`acting_session`) and rejects a bare account,
// so the web UI cannot call it and must not work around that.
//
// Every string below that a client or a repository wrote (template text, rule
// paths, tool labels, notes) is data to display as plain text, never markup and
// never an instruction.
import { request, qs } from './_base.svelte';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration`;

export interface CursorPage<T> {
  items: T[];
  next_cursor: string | null;
}

// ── Templates ────────────────────────────────────────

export interface ChecklistItem {
  text: string;
  required: boolean;
}

export interface TemplateContent {
  title_pattern: string;
  goal: string;
  deliverables: string[];
  checklist: ChecklistItem[];
}

/** One immutable version; the content fields are flattened into it by the server. */
export interface TemplateVersion extends TemplateContent {
  template_id: string;
  version: number;
  created_by_user_id: number;
  created_at: string;
  origin: string;
}

export interface TemplateView {
  id: string;
  name: string;
  latest_version: number;
  created_by_user_id: number;
  created_at: string;
  updated_at: string;
  /** The requested version (the latest by default). */
  version: TemplateVersion;
  origin: string;
}

export interface TemplateCreateInput extends TemplateContent {
  client_template_id: string;
  name: string;
}

export interface TemplateVersionInput extends TemplateContent {
  client_version_id: string;
  /** The `latest_version` the editor saw. A stale value is a `VERSION_CONFLICT`. */
  expected_version: number;
}

export interface TemplateLink {
  task_id: string;
  template_id: string;
  template_version: number;
  snapshot: {
    template_id: string;
    template_name: string;
    template_version: number;
    title_pattern: string;
    goal: string;
    deliverables: string[];
    checklist: ChecklistItem[];
    params: Record<string, string>;
    rendered_title: string;
  };
  rules_commit: string | null;
  creator_kind: string;
  creator_id: string;
  created_at: string;
  template_latest_version: number | null;
  newer_version_available: boolean;
  origin: string;
}

// ── Search ───────────────────────────────────────────

export const SEARCH_KINDS = ['task', 'question', 'decision', 'handoff', 'message', 'work_package'] as const;
export type SearchKind = (typeof SEARCH_KINDS)[number];

export interface SearchHit {
  kind: string;
  id: string;
  task_id: string;
  created_at: string;
  origin: string;
}

export interface SearchPage {
  items: SearchHit[];
  next_cursor: string | null;
  queried_at: string;
  consistency: string;
}

// ── Rules ────────────────────────────────────────────

export interface RuleEntry {
  path: string;
  blob_sha: string;
  size: number;
  oversize: boolean;
  /** Directory the rules govern; `''` is the whole repository. */
  applies_to: string;
  /** `manifest` | `default_discovery` */
  source: string;
  origin: string;
  /** API path of the blob, pinned to `resolved_commit`. */
  read_url: string;
}

export interface SkippedRule {
  path: string;
  /** `symlink` | `submodule` | `not_a_file` */
  reason: string;
}

export interface RulesResponse {
  requested_ref: string;
  resolved_commit: string | null;
  path: string;
  discovery_version: number;
  manifest: { status: string; error: string | null };
  entries: RuleEntry[];
  skipped: SkippedRule[];
  truncated: boolean;
  next_cursor: string | null;
}

// ── Escalation settings ──────────────────────────────

export interface QuietHours {
  start_minute: number;
  end_minute: number;
}

export interface EscalationSettings {
  enabled: boolean;
  blocked_after_secs: number;
  question_after_secs: number;
  handoff_after_secs: number;
  /** 0 = remind once per episode. */
  repeat_after_secs: number;
  quiet_hours: QuietHours | null;
  timezone: string;
  /** Send as `expected_version`; 0 = nothing stored yet. */
  version: number;
  /** `default` (nothing stored, escalation off) | `stored` */
  source: string;
  updated_at: string | null;
  updated_by_user_id: number | null;
}

export interface EscalationInput {
  expected_version: number;
  enabled: boolean;
  blocked_after_secs: number;
  question_after_secs: number;
  handoff_after_secs: number;
  repeat_after_secs: number;
  quiet_hours: QuietHours | null;
}

// ── Declarations ─────────────────────────────────────

export interface Declaration {
  session_id: string;
  version: number;
  tools: string[];
  can_run_tests: string;
  network: string;
  network_notes: string;
  notes: string;
  declared_at: string;
  age_seconds: number;
  stale: boolean;
  stale_reasons: string[];
  stale_after_seconds: number;
  session_state: string;
  /** Always `user_content`. */
  source: string;
  /** Always `none`: a declaration never authorizes anything. */
  authorization: string;
  meaning: string;
}

// ── Deletion impact ──────────────────────────────────

export interface CollabCounts {
  sessions: number;
  tasks: number;
  messages: number;
  handoffs: number;
  takeovers: number;
  questions: number;
  answers: number;
  work_packages: number;
}

export interface RepoImpact {
  repo_id: number;
  records: CollabCounts;
  on_repository_delete: string;
  on_repository_row_removal: string;
  git_data_retention_days: number;
  retention_policy: string;
  as_of: string;
}

export const collabOps = {
  // templates
  listTemplates: (owner: string, repo: string, p: { limit?: number; cursor?: string | null } = {}) =>
    request<CursorPage<TemplateView>>(`${base(owner, repo)}/templates${qs({ limit: p.limit, cursor: p.cursor })}`),
  getTemplate: (owner: string, repo: string, id: string, version?: number) =>
    request<TemplateView>(`${base(owner, repo)}/templates/${encodeURIComponent(id)}${qs({ version })}`),
  createTemplate: (owner: string, repo: string, input: TemplateCreateInput) =>
    request<TemplateView>(`${base(owner, repo)}/templates`, { method: 'POST', body: JSON.stringify(input) }),
  newTemplateVersion: (owner: string, repo: string, id: string, input: TemplateVersionInput) =>
    request<TemplateView>(`${base(owner, repo)}/templates/${encodeURIComponent(id)}/versions`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),
  getTaskTemplate: (owner: string, repo: string, taskId: string) =>
    request<TemplateLink>(`${base(owner, repo)}/tasks/${encodeURIComponent(taskId)}/template`),

  // search
  search: (owner: string, repo: string, p: { kind: string; q: string; limit?: number; cursor?: string | null }) =>
    request<SearchPage>(`${base(owner, repo)}/search${qs({ kind: p.kind, q: p.q, limit: p.limit, cursor: p.cursor })}`),

  // rules
  rules: (owner: string, repo: string, p: { ref?: string; path?: string } = {}) =>
    request<RulesResponse>(`${base(owner, repo)}/rules${qs({ ref: p.ref, path: p.path })}`),

  // escalation
  getEscalation: (owner: string, repo: string) =>
    request<EscalationSettings>(`${base(owner, repo)}/escalation-settings`),
  putEscalation: (owner: string, repo: string, input: EscalationInput) =>
    request<EscalationSettings>(`${base(owner, repo)}/escalation-settings`, {
      method: 'PUT',
      body: JSON.stringify(input),
    }),

  // declarations
  listDeclarations: (
    owner: string,
    repo: string,
    p: { limit?: number; cursor?: string | null; stale_after_seconds?: number } = {},
  ) =>
    request<CursorPage<Declaration>>(
      `${base(owner, repo)}/declarations${qs({ limit: p.limit, cursor: p.cursor, stale_after_seconds: p.stale_after_seconds })}`,
    ),

  // deletion impact (repository admins only)
  deletionImpact: (owner: string, repo: string) => request<RepoImpact>(`${base(owner, repo)}/deletion-impact`),
};
