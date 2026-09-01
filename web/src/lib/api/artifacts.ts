import { request } from './_base.svelte';
import type { Artifact } from '$lib/types/entities';

export const artifacts = {
  listByPipeline: (owner: string, repo: string, pipelineId: number) =>
    request<Artifact[]>(`/repos/${owner}/${repo}/pipelines/${pipelineId}/artifacts`),
};
