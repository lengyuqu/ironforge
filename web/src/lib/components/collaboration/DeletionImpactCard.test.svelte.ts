import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { ApiError } from "$lib/api/_base.svelte";

vi.mock("$lib/components/toast.svelte", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("$lib/i18n", () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === "object" ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

const api = vi.hoisted(() => ({ deletionImpact: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({ ...(await orig<typeof import("$lib/api/collabOps")>()), collabOps: api }));

import DeletionImpactCard from "./DeletionImpactCard.svelte";

const impact = (over = {}) => ({
  repo_id: 1,
  records: { sessions: 2, tasks: 3, messages: 41, handoffs: 0, takeovers: 1, questions: 5, answers: 4, work_packages: 6 },
  on_repository_delete: "kept",
  on_repository_row_removal: "cascade",
  git_data_retention_days: 30,
  retention_policy: "none",
  as_of: "2026-11-06T00:00:00Z",
  ...over,
});

describe("DeletionImpactCard.svelte", () => {
  beforeEach(() => {
    api.deletionImpact.mockReset().mockResolvedValue(impact());
  });

  it("shows the counts, labelled as a read-only preview", async () => {
    render(DeletionImpactCard, { props: { owner: "alice", repo: "demo" } });
    const table = await screen.findByTestId("impact-counts");
    expect(table).toHaveTextContent("41");
    expect(table).toHaveTextContent("collab_ops.impact.kind.work_packages");
    expect(screen.getByTestId("impact-preview")).toHaveTextContent("collab_ops.impact.preview_banner");
    expect(screen.getByText("collab_ops.impact.on_delete_value.kept")).toBeInTheDocument();
    expect(screen.getByText("collab_ops.impact.on_row_removal_value.cascade")).toBeInTheDocument();
    // a preview must not offer any destructive control
    expect(screen.queryByRole("button", { name: /delete|remove/i })).toBeNull();
  });

  it("tells a non-admin plainly that it is unavailable, without showing numbers", async () => {
    api.deletionImpact.mockRejectedValue(new ApiError("repository admin access required", 403));
    render(DeletionImpactCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByTestId("impact-forbidden")).toBeInTheDocument();
    expect(screen.queryByTestId("impact-counts")).toBeNull();
  });

  it("shows other errors and re-reads on refresh", async () => {
    api.deletionImpact.mockRejectedValueOnce(new ApiError("boom", 500)).mockResolvedValueOnce(impact());
    render(DeletionImpactCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.impact.refresh" }));
    expect(await screen.findByTestId("impact-counts")).toBeInTheDocument();
  });

  it("shows unknown policy values raw", async () => {
    api.deletionImpact.mockResolvedValue(impact({ retention_policy: "90_days" }));
    render(DeletionImpactCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByText("90_days")).toBeInTheDocument();
  });
});
