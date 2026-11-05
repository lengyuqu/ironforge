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

const api = vi.hoisted(() => ({ search: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({
  ...(await orig<typeof import("$lib/api/collabOps")>()),
  collabOps: api,
}));

import Page from "./+page.svelte";

const hit = (over = {}) => ({ kind: "task", id: "aaaaaaaa-1111", task_id: "aaaaaaaa-1111", created_at: "2026-11-05T00:00:00Z", origin: "user_content", ...over });
const page1 = (items: unknown[], next: string | null = null) => ({ items, next_cursor: next, queried_at: "2026-11-06T00:00:00Z", consistency: "live" });

async function search(text: string) {
  await fireEvent.input(screen.getByTestId("search-q"), { target: { value: text } });
  await fireEvent.click(screen.getByRole("button", { name: "collab_ops.search.go" }));
}

describe("search page", () => {
  beforeEach(() => {
    api.search.mockReset();
  });

  it("does nothing until there is text", async () => {
    render(Page);
    expect(screen.getByRole("button", { name: "collab_ops.search.go" })).toBeDisabled();
    expect(api.search).not.toHaveBeenCalled();
  });

  it("refuses LIKE metacharacters before calling the server", async () => {
    render(Page);
    await fireEvent.input(screen.getByTestId("search-q"), { target: { value: "50%" } });
    expect(screen.getByText("collab_ops.search.bad_chars")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "collab_ops.search.go" })).toBeDisabled();
  });

  it("searches the chosen kind and shows kind, id and a link to the task, with no count", async () => {
    api.search.mockResolvedValue(page1([hit({ kind: "question", id: "qqqqqqqq-2", task_id: "tttttttt-3" })]));
    render(Page);
    const sel = screen.getByTestId("search-kind") as HTMLSelectElement;
    // happy-dom does not implement select:checked, which Svelte's bind:value reads.
    const original = HTMLSelectElement.prototype.querySelector;
    const spy = vi.spyOn(HTMLSelectElement.prototype, "querySelector").mockImplementation(function (this: HTMLSelectElement, selector: string) {
      return selector === ":checked" ? (this.options[this.selectedIndex] ?? null) : original.call(this, selector);
    });
    try {
      await fireEvent.change(sel, { target: { value: "question" } });
    } finally {
      spy.mockRestore();
    }
    await search("  migration ");
    await waitFor(() => expect(api.search).toHaveBeenCalledWith("alice", "demo", expect.objectContaining({ kind: "question", q: "migration" })));
    const list = await screen.findByTestId("search-results");
    expect(list).toHaveTextContent("collab_ops.search.kind.question");
    expect(list).toHaveTextContent("qqqqqqqq");
    expect(screen.getByRole("link", { name: /collab.open_task/ })).toHaveAttribute("href", "/alice/demo/collaboration/tasks/tttttttt-3");
    expect(screen.getByText("collab_ops.search.no_snippets")).toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/total|\b\d+ (results|hits)\b/i);
  });

  it("says no visible match without claiming anything about hidden records", async () => {
    api.search.mockResolvedValue(page1([]));
    render(Page);
    await search("nothing");
    expect(await screen.findByText("collab_ops.search.none")).toBeInTheDocument();
    expect(screen.getByText("collab_ops.search.none_hint")).toBeInTheDocument();
  });

  it("pages with the cursor and keeps the original search terms", async () => {
    api.search.mockResolvedValueOnce(page1([hit({ id: "a1", task_id: "a1" })], "CUR")).mockResolvedValueOnce(page1([hit({ id: "b2", task_id: "b2" })]));
    render(Page);
    await search("fix");
    await fireEvent.input(screen.getByTestId("search-q"), { target: { value: "edited after" } });
    await fireEvent.click(await screen.findByRole("button", { name: "collab.load_more" }));
    await waitFor(() => expect(api.search).toHaveBeenCalledTimes(2));
    expect(api.search.mock.calls[1][2]).toMatchObject({ q: "fix", cursor: "CUR" });
    await waitFor(() => expect(screen.getAllByRole("link", { name: /collab.open_task/ })).toHaveLength(2));
  });

  it("shows the server's error message", async () => {
    api.search.mockRejectedValue(new ApiError("q must be 1..256 bytes", 400));
    render(Page);
    await search("x");
    expect(await screen.findByText(/q must be 1..256 bytes/)).toBeInTheDocument();
  });

  it("renders the searched text inertly", async () => {
    const hostile = "<img src=x onerror=1>";
    api.search.mockResolvedValue(page1([]));
    const { container } = render(Page);
    await search(hostile);
    await screen.findByText("collab_ops.search.none");
    expect(container.querySelector("img")).toBeNull();
  });
});
