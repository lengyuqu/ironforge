import { request, qs, type PaginatedResponse } from './_base.svelte';
import type { Issue, IssueComment, Label } from '$lib/types/entities';

/** Raw wire shape of IssueResponse before labels normalisation. */
type IssueResponse = Omit<Issue, 'labels'> & { labels?: string | string[] | null };

function parseIssueLabels(labels: string | string[] | undefined | null): string[] {
  if (Array.isArray(labels)) {
    return labels;
  }

  if (!labels || typeof labels !== 'string') {
    return [];
  }

  try {
    const parsed = JSON.parse(labels);
    if (Array.isArray(parsed)) {
      return parsed.filter((value) => typeof value === 'string');
    }
  } catch {
    // Older rows may contain comma-separated label names.
  }

  return labels
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

function normalizeIssue<T extends { labels?: string | string[] | null }>(issue: T): Omit<T, 'labels'> & { labels: string[] } {
  return {
    ...issue,
    labels: parseIssueLabels(issue.labels),
  };
}

export type IssueTemplate = {
  name: string;
  title: string;
  about: string;
  labels: string[];
  assignees: string[];
  ref: string;
  content: string;
  file_name: string;
};

export type IssueConfig = {
  blank_issues_enabled: boolean;
  contact_links: Array<{ name: string; url: string; about: string }>;
};

export const issues = {
  templates: (owner: string, repo: string) =>
    request<IssueTemplate[]>(`/repos/${owner}/${repo}/issue_templates`),
  templateConfig: (owner: string, repo: string) =>
    request<IssueConfig>(`/repos/${owner}/${repo}/issue_config`),
  list: (owner: string, repo: string, state?: string, page?: number, perPage?: number, labels?: string, assignee?: string) => {
    return request<PaginatedResponse<IssueResponse>>(`/repos/${owner}/${repo}/issues${qs({ state, page, per_page: perPage, labels, assignee })}`)
      .then((response) => ({
        ...response,
        data: response.data.map(normalizeIssue),
      }));
  },
  get: (owner: string, repo: string, number: number): Promise<Issue> =>
    request<IssueResponse>(`/repos/${owner}/${repo}/issues/${number}`).then(normalizeIssue),
  create: (owner: string, repo: string, title: string, body?: string, labels?: string[], assignees?: string[]): Promise<Issue> =>
    request<IssueResponse>(`/repos/${owner}/${repo}/issues`, {
      method: 'POST',
      body: JSON.stringify({ title, body, labels, assignees }),
    }).then(normalizeIssue),
  update: (owner: string, repo: string, number: number, data: Record<string, unknown>): Promise<Issue> =>
    request<IssueResponse>(`/repos/${owner}/${repo}/issues/${number}`, {
      method: 'PATCH',
      body: JSON.stringify(data),
    }).then(normalizeIssue),
  comments: (owner: string, repo: string, number: number) =>
    request<IssueComment[]>(`/repos/${owner}/${repo}/issues/${number}/comments`),
  labels: (owner: string, repo: string, number: number) =>
    request<Label[]>(`/repos/${owner}/${repo}/issues/${number}/labels`),
  addComment: (owner: string, repo: string, number: number, body: string) =>
    request<IssueComment>(`/repos/${owner}/${repo}/issues/${number}/comments`, {
      method: 'POST',
      body: JSON.stringify({ body }),
    }),
  listReactions: (owner: string, repo: string, issueNumber: number) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/${issueNumber}/reactions`),
  addReaction: (owner: string, repo: string, issueNumber: number, content: string) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/${issueNumber}/reactions`, {
      method: 'POST',
      body: JSON.stringify({ content }),
    }),
  removeReaction: (owner: string, repo: string, issueNumber: number, content: string) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/${issueNumber}/reactions`, {
      method: 'DELETE',
      body: JSON.stringify({ content }),
    }),
  listCommentReactions: (owner: string, repo: string, commentId: number) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/comments/${commentId}/reactions`),
  addCommentReaction: (owner: string, repo: string, commentId: number, content: string) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/comments/${commentId}/reactions`, {
      method: 'POST',
      body: JSON.stringify({ content }),
    }),
  removeCommentReaction: (owner: string, repo: string, commentId: number, content: string) =>
    request<ReactionSummary[]>(`/repos/${owner}/${repo}/issues/comments/${commentId}/reactions`, {
      method: 'DELETE',
      body: JSON.stringify({ content }),
    }),
  listAssignees: (owner: string, repo: string, number: number) =>
    request<{ assignees: string[] }>(`/repos/${owner}/${repo}/issues/${number}/assignees`),
  setAssignees: (owner: string, repo: string, number: number, assignees: string[]) =>
    request<{ assignees: string[] }>(`/repos/${owner}/${repo}/issues/${number}/assignees`, {
      method: 'PUT',
      body: JSON.stringify({ assignees }),
    }),
};

export type ReactionSummary = {
  content: string;
  count: number;
  reacted_by_me: boolean;
};
