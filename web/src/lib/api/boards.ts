import { request } from './_base.svelte';
import type { Board, BoardColumn, BoardCard, BoardFull } from '$lib/types/entities';

export const boards = {
  list: (owner: string, repo: string) =>
    request<Board[]>(`/repos/${owner}/${repo}/boards`),
  get: (owner: string, repo: string, id: number) =>
    request<BoardFull>(`/repos/${owner}/${repo}/boards/${id}`),
  create: (owner: string, repo: string, data: { name: string; description?: string }) =>
    request<Board>(`/repos/${owner}/${repo}/boards`, { method: 'POST', body: JSON.stringify(data) }),
  update: (owner: string, repo: string, id: number, data: { name?: string; description?: string }) =>
    request<Board>(`/repos/${owner}/${repo}/boards/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  delete: (owner: string, repo: string, id: number) =>
    request<void>(`/repos/${owner}/${repo}/boards/${id}`, { method: 'DELETE' }),
  createColumn: (owner: string, repo: string, boardId: number, data: { name: string }) =>
    request<BoardColumn>(`/repos/${owner}/${repo}/boards/${boardId}/columns`, { method: 'POST', body: JSON.stringify(data) }),
  updateColumn: (owner: string, repo: string, boardId: number, colId: number, data: { name?: string }) =>
    request<BoardColumn>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}`, { method: 'PATCH', body: JSON.stringify(data) }),
  deleteColumn: (owner: string, repo: string, boardId: number, colId: number) =>
    request<void>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}`, { method: 'DELETE' }),
  createCard: (owner: string, repo: string, boardId: number, colId: number, data: { note?: string; issue_id?: number }) =>
    request<BoardCard>(`/repos/${owner}/${repo}/boards/${boardId}/columns/${colId}/cards`, { method: 'POST', body: JSON.stringify(data) }),
  updateCard: (owner: string, repo: string, boardId: number, cardId: number, data: { note?: string; issue_id?: number | null }) =>
    request<BoardCard>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}`, { method: 'PATCH', body: JSON.stringify(data) }),
  moveCard: (owner: string, repo: string, boardId: number, cardId: number, data: { column_id: number; position: number }) =>
    request<BoardCard>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}/move`, { method: 'POST', body: JSON.stringify(data) }),
  reorderCards: (owner: string, repo: string, boardId: number, data: { positions: [number, number][] }) =>
    request<{ status: string }>(`/repos/${owner}/${repo}/boards/${boardId}/cards/reorder`, { method: 'POST', body: JSON.stringify(data) }),
  deleteCard: (owner: string, repo: string, boardId: number, cardId: number) =>
    request<void>(`/repos/${owner}/${repo}/boards/${boardId}/cards/${cardId}`, { method: 'DELETE' }),
};
