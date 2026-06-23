import { API_BASE, request, qs, type PaginatedResponse } from './_base';

export interface ReleaseAsset {
  id: number;
  release_id: number;
  filename: string;
  size: number;
  content_type: string;
  download_count: number;
  uploader_id: number;
  created_at: string;
}

export const releases = {
  list: (owner: string, repo: string, page?: number, perPage?: number) =>
    request<PaginatedResponse<any>>(`/repos/${owner}/${repo}/releases${qs({ page, per_page: perPage })}`),
  get: (owner: string, repo: string, id: number) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`),
  create: (owner: string, repo: string, data: { tag_name: string; title: string; body?: string; target_commitish?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases`, { method: 'POST', body: JSON.stringify(data) }),
  update: (owner: string, repo: string, id: number, data: { title?: string; body?: string; is_draft?: boolean; is_prerelease?: boolean }) =>
    request<any>(`/repos/${owner}/${repo}/releases/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/releases/${id}`, { method: 'DELETE' }),
  listAssets: (owner: string, repo: string, releaseId: number) =>
    request<ReleaseAsset[]>(`/repos/${owner}/${repo}/releases/${releaseId}/assets`),
  uploadAsset: (owner: string, repo: string, releaseId: number, file: File) =>
    request<ReleaseAsset>(`/repos/${owner}/${repo}/releases/${releaseId}/assets`, {
      method: 'POST',
      headers: {
        'Content-Type': file.type || 'application/octet-stream',
        'x-asset-filename': file.name,
      },
      body: file,
    }),
  getAsset: (owner: string, repo: string, assetId: number) =>
    request<ReleaseAsset>(`/repos/${owner}/${repo}/releases/assets/${assetId}`),
  assetDownloadUrl: (owner: string, repo: string, assetId: number) =>
    `${API_BASE}/repos/${owner}/${repo}/releases/assets/${assetId}/download`,
  deleteAsset: (owner: string, repo: string, assetId: number) =>
    request<void>(`/repos/${owner}/${repo}/releases/assets/${assetId}`, { method: 'DELETE' }),
};
