// Shared repo URL builders — keep tree/blob/commit links consistent between
// the repo overview page and its sub-components.

/** Serialize ref/path into a query string (with leading '?' when non-empty). */
export function buildRepoQuery(ref: string, path: string): string {
  const params = new URLSearchParams();
  if (ref) params.set('ref', ref);
  if (path) params.set('path', path);
  const qs = params.toString();
  return qs ? `?${qs}` : '';
}

/** Link to the tree browser at the given ref/path. */
export function buildTreeHref(owner: string, repo: string, ref: string, path: string): string {
  return `/${owner}/${repo}${buildRepoQuery(ref, path)}`;
}

/** Link to a commit on the current ref. */
export function buildCommitHref(owner: string, repo: string, ref: string, sha: string): string {
  return `/${owner}/${repo}/commits/${sha}${buildRepoQuery(ref, '')}`;
}

/** URL-encode each path segment of a file path. */
export function encodeRepoPath(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

/** Link to the blob viewer for a file, preserving the current ref. */
export function buildBlobHref(owner: string, repo: string, ref: string, filePath: string): string {
  return `/${owner}/${repo}/blob/${encodeRepoPath(filePath)}${buildRepoQuery(ref, '')}`;
}
