// Cross-repository coordination groups — human-facing REST client.
//
// Contract facts the UI relies on (see
// docs/design/agent-collaboration.md §7):
//   * There is NO "list all groups of a repository" endpoint, and groups have
//     no rename / remove / restore endpoint. Only create, read, and add/remove
//     links exist.
//   * Links are listed per page and each page is re-authorized for the reader.
//     Links to repositories the reader cannot see are left out without a trace:
//     there is no total, no count and no marker for hidden links.
//   * `version` is only sent to people who can write to the anchor repository.
//   * Title, description and every link text are client-written: render as
//     plain text.
import { request, qs } from './_base.svelte';

const base = (owner: string, repo: string) =>
  `/repos/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/collaboration/groups`;

export interface GroupView {
  id: string;
  anchor_repo_id: number;
  title: string;
  description: string;
  created_by_kind: string;
  created_by_id: string;
  /** Only present when the reader may write to the anchor repository. */
  version?: number;
  created_at: string;
  origin: string;
  /** Reminder about who can read title/description (create response only). */
  notice?: string;
}

export interface LinkRepoRef {
  id: number;
  owner: string;
  name: string;
}

export interface LinkTaskRef {
  id: string;
  title: string;
  /** Declared by whoever set it; not evidence. */
  status: string;
  version: number;
  origin: string;
}

export interface LinkCommitRef {
  sha: string;
  /** `available` | `unavailable` (the object cannot be read right now). */
  commit_state: string;
}

export interface GroupLink {
  id: string;
  group_id: string;
  target_repo: LinkRepoRef;
  task: LinkTaskRef | null;
  commit: LinkCommitRef | null;
  created_by_kind: string;
  created_by_id: string;
  created_at: string;
  removed_at: string | null;
}

/** One page of the links the reader may see. Deliberately has no total. */
export interface LinkPage {
  items: GroupLink[];
  next_cursor: string | null;
  has_more: boolean;
  as_of: string;
}

export interface GroupDetail extends GroupView {
  links: LinkPage;
}

export interface AddLinkInput {
  client_request_id: string;
  target_repo_id: number;
  target_task_id?: string | null;
  commit_sha?: string | null;
  expected_group_version: number;
}

export const GROUP_LIMITS = { titleChars: 200, descriptionBytes: 16 * 1024 } as const;

export const collabGroups = {
  create: (owner: string, repo: string, input: { client_request_id: string; title: string; description: string }) =>
    request<GroupView>(base(owner, repo), { method: 'POST', body: JSON.stringify(input) }),

  get: (owner: string, repo: string, groupId: string, opts: { limit?: number } = {}) =>
    request<GroupDetail>(`${base(owner, repo)}/${encodeURIComponent(groupId)}${qs({ limit: opts.limit })}`),

  listLinks: (owner: string, repo: string, groupId: string, opts: { limit?: number; cursor?: string | null } = {}) =>
    request<LinkPage>(
      `${base(owner, repo)}/${encodeURIComponent(groupId)}/links${qs({ limit: opts.limit, cursor: opts.cursor })}`,
    ),

  addLink: (owner: string, repo: string, groupId: string, input: AddLinkInput) =>
    request<GroupLink>(`${base(owner, repo)}/${encodeURIComponent(groupId)}/links`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  removeLink: (
    owner: string,
    repo: string,
    groupId: string,
    linkId: string,
    input: { client_request_id: string; expected_group_version: number },
  ) =>
    request<GroupLink>(
      `${base(owner, repo)}/${encodeURIComponent(groupId)}/links/${encodeURIComponent(linkId)}`,
      { method: 'DELETE', body: JSON.stringify(input) },
    ),
};
