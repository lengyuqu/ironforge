import { getContext, setContext } from 'svelte';
import type { RepoInfo } from '$lib/types/entities';

export interface RepoContext {
  readonly repoInfo: RepoInfo | null;
  refresh: () => Promise<void>;
  setRepoInfo: (info: RepoInfo | null) => void;
}

const REPO_CONTEXT_KEY = Symbol('ironforge-repo-context');

export function setRepoContext(ctx: RepoContext): RepoContext {
  return setContext(REPO_CONTEXT_KEY, ctx);
}

export function getRepoContext(): RepoContext | undefined {
  return getContext<RepoContext | undefined>(REPO_CONTEXT_KEY);
}
