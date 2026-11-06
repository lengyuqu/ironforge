import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { ApiError } from "$lib/api/_base.svelte";

const pageStore = vi.hoisted(() => {
  const value = { params: { owner: "alice", repo: "demo" } };
  return { page: { subscribe(fn: (v: typeof value) => void) { fn(value); return () => {}; } } };
});
vi.mock("$app/stores", () => ({ page: pageStore.page }));
vi.mock("$lib/components/RepoHeader.svelte", async () => ({ default: (await import("../../../../../test-stubs/OpsEmptyStub.svelte")).default }));
vi.mock("$lib/components/toast.svelte", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("$lib/i18n", () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === "object" ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

const api = vi.hoisted(() => ({ getEscalation: vi.fn(), putEscalation: vi.fn(), listDeclarations: vi.fn(), deletionImpact: vi.fn(), loadSessionMap: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({ ...(await orig<typeof import("$lib/api/collabOps")>()), collabOps: api }));
vi.mock("$lib/api/collaboration", () => ({ loadSessionMap: (...a: unknown[]) => api.loadSessionMap(...a) }));

import Page from "./+page.svelte";

const stored = { enabled: true, blocked_after_secs: 3600, question_after_secs: 3600, handoff_after_secs: 900, repeat_after_secs: 0, quiet_hours: null, timezone: "UTC", version: 2, source: "stored", updated_at: "2026-11-06T00:00:00Z", updated_by_user_id: 1 };
const impact = { repo_id: 1, records: { sessions: 1, tasks: 2, messages: 3, handoffs: 0, takeovers: 0, questions: 0, answers: 0, work_packages: 0 }, on_repository_delete: "kept", on_repository_row_removal: "cascade", git_data_retention_days: 30, retention_policy: "none", as_of: "2026-11-06T00:00:00Z" };

describe("settings page", () => {
  beforeEach(() => {
    api.getEscalation.mockReset().mockResolvedValue(stored);
    api.listDeclarations.mockReset().mockResolvedValue({ items: [], next_cursor: null });
    api.loadSessionMap.mockReset().mockResolvedValue(new Map());
    api.deletionImpact.mockReset().mockResolvedValue(impact);
  });

  it("shows the three sections and the shared nav", async () => {
    render(Page);
    expect(await screen.findByTestId("escalation-view")).toBeInTheDocument();
    expect(await screen.findByTestId("declarations-claims-note")).toBeInTheDocument();
    expect(await screen.findByTestId("impact-counts")).toBeInTheDocument();
    expect(screen.getByTestId("collab-nav")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "collab_ops.escalation.edit" })).not.toBeDisabled();
  });

  it("explains the admin-only parts to a non-admin, while the readable parts still show", async () => {
    api.deletionImpact.mockRejectedValue(new ApiError("repository admin access required", 403));
    render(Page);
    expect(await screen.findByTestId("impact-forbidden")).toBeInTheDocument();
    expect(await screen.findByTestId("escalation-not-admin")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "collab_ops.escalation.edit" })).toBeDisabled();
    expect(screen.getByTestId("escalation-view")).toBeInTheDocument();
    expect(screen.getByTestId("declarations-claims-note")).toBeInTheDocument();
  });

  it("one section failing does not break the others", async () => {
    api.listDeclarations.mockRejectedValue(new ApiError("boom", 500));
    render(Page);
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
    expect(await screen.findByTestId("escalation-view")).toBeInTheDocument();
    expect(await screen.findByTestId("impact-counts")).toBeInTheDocument();
  });
});
