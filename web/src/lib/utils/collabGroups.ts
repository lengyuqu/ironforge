// Pure helpers for the coordination-group pages. No DOM access, so the input
// rules and the browser-local "recently opened" list are unit-testable.
import { GROUP_LIMITS } from '$lib/api/collabGroups';

/** `owner/name` → parts, or null when it is not exactly that shape. */
export function parseRepoRef(raw: string): { owner: string; name: string } | null {
  const text = raw.trim();
  const parts = text.split('/');
  if (parts.length !== 2) return null;
  const [owner, name] = parts;
  if (!owner || !name || /\s/.test(text)) return null;
  return { owner, name };
}

/** A full 40- or 64-digit commit id (the server lower-cases it). */
export function isFullSha(raw: string): boolean {
  return /^(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})$/.test(raw.trim());
}

export type LinkProblem = 'target_invalid' | 'need_one' | 'sha_invalid';

/** Why the add-link form cannot be sent yet, or null. Mirrors the server's rules. */
export function linkProblem(target: string, taskId: string, sha: string): LinkProblem | null {
  if (!parseRepoRef(target)) return 'target_invalid';
  if (!taskId.trim() && !sha.trim()) return 'need_one';
  if (sha.trim() && !isFullSha(sha)) return 'sha_invalid';
  return null;
}

export type GroupProblem = 'title_required' | 'title_too_long' | 'description_too_long';

export function groupProblem(title: string, description: string): GroupProblem | null {
  const t = title.trim();
  if (!t) return 'title_required';
  if ([...t].length > GROUP_LIMITS.titleChars) return 'title_too_long';
  if (new TextEncoder().encode(description).length > GROUP_LIMITS.descriptionBytes) return 'description_too_long';
  return null;
}

// ── "Recently opened groups" ─────────────────────────────────────────
// The server has no endpoint that lists a repository's groups, so the UI can
// only remember the groups this browser created or opened. This is a per-viewer
// convenience (localStorage), never a list of all groups; every read and write
// is guarded because storage can be blocked or hold anything.

export interface RememberedGroup {
  id: string;
  title: string;
}

const MAX_REMEMBERED = 20;
const storageKey = (owner: string, repo: string) => `ironforge:collab-groups:${owner}/${repo}`;

export function loadRemembered(owner: string, repo: string): RememberedGroup[] {
  try {
    const raw = localStorage.getItem(storageKey(owner, repo));
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((x): x is RememberedGroup => !!x && typeof x.id === 'string' && typeof x.title === 'string')
      .slice(0, MAX_REMEMBERED);
  } catch {
    return [];
  }
}

function save(owner: string, repo: string, list: RememberedGroup[]) {
  try {
    localStorage.setItem(storageKey(owner, repo), JSON.stringify(list.slice(0, MAX_REMEMBERED)));
  } catch {
    // Storage unavailable: the list simply does not persist.
  }
}

export function remember(owner: string, repo: string, group: RememberedGroup) {
  const rest = loadRemembered(owner, repo).filter((g) => g.id !== group.id);
  save(owner, repo, [group, ...rest]);
}

export function forget(owner: string, repo: string, id: string) {
  save(owner, repo, loadRemembered(owner, repo).filter((g) => g.id !== id));
}
