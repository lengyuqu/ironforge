import { request, qs, type PaginatedResponse } from './_base.svelte';
import type {
  Pipeline,
  PipelineDetailResponse,
  PipelineJob,
} from '$lib/types/entities';

export const pipelines = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<Pipeline>>(`/repos/${owner}/${repo}/pipelines${qs({ page, per_page: perPage })}`),
  get: (owner: string, repo: string, id: number) =>
    request<PipelineDetailResponse>(`/repos/${owner}/${repo}/pipelines/${id}`),
  trigger: (owner: string, repo: string, ref?: string) =>
    request<{ id: number; status: string; commit_sha: string; ref_name: string }>(`/repos/${owner}/${repo}/pipelines`, {
      method: 'POST',
      body: JSON.stringify({ ref }),
    }),
  retry: (owner: string, repo: string, id: number) =>
    request<{ id: number; status: string }>(`/repos/${owner}/${repo}/pipelines/${id}/retry`, { method: 'POST' }),
  cancel: (owner: string, repo: string, id: number) =>
    request<{ id: number; status: string }>(`/repos/${owner}/${repo}/pipelines/${id}/cancel`, { method: 'POST' }),
  job: (owner: string, repo: string, pipelineId: number, jobId: number) =>
    request<PipelineJob>(`/repos/${owner}/${repo}/pipelines/${pipelineId}/jobs/${jobId}`),
  play: (owner: string, repo: string, pipelineId: number, jobId: number) =>
    request<{ id: number; pipeline_id: number; status: string }>(`/repos/${owner}/${repo}/pipelines/${pipelineId}/jobs/${jobId}/play`, { method: 'POST' }),
  approve: (owner: string, repo: string, pipelineId: number, jobId: number) =>
    request<{ job_id: number; approvals: number; required_approvals: number; released: boolean }>(`/repos/${owner}/${repo}/pipelines/${pipelineId}/jobs/${jobId}/approve`, { method: 'POST' }),
};
