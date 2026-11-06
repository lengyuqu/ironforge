import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { ApiError } from "$lib/api/_base.svelte";

const pageStore = vi.hoisted(() => {
  const value = { params: { owner: "alice", repo: "demo" } };
  return {
    page: {
      subscribe(fn: (v: typeof value) => void) {
        fn(value);
        return () => {};
      },
    },
  };
});
vi.mock("$app/stores", () => ({ page: pageStore.page }));
vi.mock("$lib/components/RepoHeader.svelte", async () => ({ default: (await import("../../../../../test-stubs/OpsEmptyStub.svelte")).default }));
vi.mock("$lib/components/toast.svelte", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("$lib/i18n", () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === "object" ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

const api = vi.hoisted(() => ({ listTemplates: vi.fn(), getTemplate: vi.fn(), createTemplate: vi.fn(), newTemplateVersion: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({
  ...(await orig<typeof import("$lib/api/collabOps")>()),
  collabOps: api,
}));

import Page from "./+page.svelte";
import type { TemplateView } from "$lib/api/collabOps";

const HOSTILE = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>[click](javascript:alert(1))';

function tpl(over: Partial<TemplateView> = {}, versionNo = 2): TemplateView {
  return {
    id: "tpl-1",
    name: "Bugfix",
    latest_version: versionNo,
    created_by_user_id: 1,
    created_at: "2026-11-05T00:00:00Z",
    updated_at: "2026-11-05T00:00:00Z",
    origin: "user_content",
    version: {
      template_id: "tpl-1",
      version: versionNo,
      title_pattern: "Fix {bug}",
      goal: "Find and fix it",
      deliverables: ["patch"],
      checklist: [{ text: "tests pass", required: true }],
      created_by_user_id: 1,
      created_at: "2026-11-05T00:00:00Z",
      origin: "user_content",
    },
    ...over,
  };
}

describe("templates page", () => {
  beforeEach(() => {
    Object.values(api).forEach((f) => f.mockReset());
    api.listTemplates.mockResolvedValue({ items: [tpl()], next_cursor: null });
  });

  it("says task creation from a template is not available here and offers no such control", async () => {
    render(Page);
    expect(await screen.findByTestId("from-template-notice")).toHaveTextContent("collab_ops.templates.no_task_body");
    expect(screen.queryByRole("button", { name: /from.template|create.task/i })).toBeNull();
  });

  it("lists templates with their latest version and the shared nav", async () => {
    render(Page);
    expect(await screen.findByText("Bugfix")).toBeInTheDocument();
    expect(screen.getByText("v2")).toBeInTheDocument();
    expect(screen.getByTestId("collab-nav")).toBeInTheDocument();
  });

  it("shows an empty state", async () => {
    api.listTemplates.mockResolvedValue({ items: [], next_cursor: null });
    render(Page);
    expect(await screen.findByText("collab_ops.templates.empty")).toBeInTheDocument();
  });

  it("shows a read denied state for 403", async () => {
    api.listTemplates.mockRejectedValue(new ApiError("denied", 403));
    render(Page);
    expect(await screen.findByText("collab.access_denied")).toBeInTheDocument();
  });

  it("loads every version when opened and can show an older one, labelled as older", async () => {
    api.getTemplate.mockImplementation(async (_o: string, _r: string, _id: string, v: number) =>
      tpl({ latest_version: 2, version: { ...tpl().version, version: v, goal: `goal of v${v}` } }, 2),
    );
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.templates.details" }));
    await waitFor(() => expect(screen.getByRole("tab", { name: "v1" })).toBeInTheDocument());
    // v2 is the list's own copy: only v1 is fetched
    expect(api.getTemplate).toHaveBeenCalledTimes(1);
    expect(api.getTemplate.mock.calls[0].slice(2)).toEqual(["tpl-1", 1]);
    await fireEvent.click(screen.getByRole("tab", { name: "v1" }));
    expect(screen.getByText("goal of v1")).toBeInTheDocument();
    expect(screen.getByText(/collab_ops.templates.older_version/)).toBeInTheDocument();
  });

  it("renders hostile template text as inert text everywhere", async () => {
    const h = tpl({ name: HOSTILE, version: { ...tpl().version, title_pattern: HOSTILE, goal: HOSTILE, deliverables: [HOSTILE], checklist: [{ text: HOSTILE, required: true }] } });
    api.listTemplates.mockResolvedValue({ items: [h], next_cursor: null });
    api.getTemplate.mockResolvedValue({ ...h, version: { ...h.version, version: 1 } });
    const { container } = render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.templates.details" }));
    await screen.findByTestId("template-content");
    expect(container.querySelector("img, script, a[href^='javascript']")).toBeNull();
    expect(screen.getAllByText(HOSTILE).length).toBeGreaterThanOrEqual(3);
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("reports a conflict from the editor and reloads the list", async () => {
    api.newTemplateVersion.mockRejectedValue(new ApiError("template changed", 409, "VERSION_CONFLICT"));
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.templates.edit" }));
    await fireEvent.input(screen.getByLabelText(/^collab_ops.templates.goal/), { target: { value: "new goal" } });
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.templates.save_version" }));
    expect(await screen.findByText(/template changed/)).toBeInTheDocument();
    api.listTemplates.mockClear();
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.templates.reload" }));
    await waitFor(() => expect(api.listTemplates).toHaveBeenCalled());
  });

  it("surfaces the server's 403 when a non-admin tries to create", async () => {
    api.createTemplate.mockRejectedValue(new ApiError("repository admin access required", 403));
    render(Page);
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.templates.new" }));
    await fireEvent.input(screen.getByLabelText(/^collab_ops.templates.name/), { target: { value: "T" } });
    await fireEvent.input(screen.getByLabelText(/^collab_ops.templates.title_pattern/), { target: { value: "x" } });
    await fireEvent.input(screen.getByLabelText(/^collab_ops.templates.goal/), { target: { value: "g" } });
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.templates.create" }));
    expect(await screen.findByText(/repository admin access required/)).toBeInTheDocument();
  });
});
