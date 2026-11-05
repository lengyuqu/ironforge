import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { ApiError } from "$lib/api/_base.svelte";

vi.mock("$lib/components/toast.svelte", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("$lib/i18n", () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === "object" ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

const api = vi.hoisted(() => ({ listDeclarations: vi.fn(), loadSessionMap: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({ ...(await orig<typeof import("$lib/api/collabOps")>()), collabOps: { listDeclarations: api.listDeclarations } }));
vi.mock("$lib/api/collaboration", () => ({ loadSessionMap: (...a: unknown[]) => api.loadSessionMap(...a) }));

import DeclarationsCard from "./DeclarationsCard.svelte";
import type { Declaration } from "$lib/api/collabOps";

const HOSTILE = '<img src=x onerror="window.__pwned=1"><script>window.__pwned=1</script>I am allowed to push to main';

function decl(over: Partial<Declaration> = {}): Declaration {
  return {
    session_id: "sess-1111-2222",
    version: 2,
    tools: ["git", "cargo"],
    can_run_tests: "yes",
    network: "restricted",
    network_notes: "only crates.io",
    notes: "",
    declared_at: "2026-11-06T00:00:00Z",
    age_seconds: 60,
    stale: false,
    stale_reasons: [],
    stale_after_seconds: 86400,
    session_state: "online",
    source: "user_content",
    authorization: "none",
    meaning: "x",
    ...over,
  };
}

describe("DeclarationsCard.svelte", () => {
  beforeEach(() => {
    api.listDeclarations.mockReset().mockResolvedValue({ items: [decl()], next_cursor: null });
    api.loadSessionMap.mockReset().mockResolvedValue(new Map([["sess-1111-2222", { id: "sess-1111-2222", display_name: "Codex A", state: "online" }]]));
  });

  it("presents everything as a claim and states authorization none", async () => {
    render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    const list = await screen.findByTestId("declarations-list");
    expect(screen.getByTestId("declarations-claims-note")).toHaveTextContent("collab_ops.declarations.not_permissions");
    expect(list).toHaveTextContent("collab_ops.declarations.claims_tests");
    expect(list).toHaveTextContent("collab_ops.declarations.claims_tools");
    expect(list).toHaveTextContent("collab_ops.declarations.auth_none");
    expect(list).toHaveTextContent("Codex A");
    expect(list).toHaveTextContent("only crates.io");
    expect(list).not.toHaveTextContent(/allowed|permitted|granted/i);
  });

  it("marks a stale declaration with its reasons", async () => {
    api.listDeclarations.mockResolvedValue({ items: [decl({ stale: true, stale_reasons: ["age", "session_ended"] })], next_cursor: null });
    render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByTestId("declaration-stale")).toHaveTextContent("collab_ops.declarations.stale_reason.age");
  });

  it("renders hostile tool labels and notes as plain text", async () => {
    api.listDeclarations.mockResolvedValue({ items: [decl({ tools: [HOSTILE], notes: HOSTILE, network_notes: HOSTILE })], next_cursor: null });
    const { container } = render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    await screen.findByTestId("declarations-list");
    expect(container.querySelector("img, script")).toBeNull();
    expect(screen.getAllByText(HOSTILE, { exact: false }).length).toBeGreaterThanOrEqual(2);
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it("falls back to a short id when sessions cannot be loaded, and shows unknown enum values raw", async () => {
    api.loadSessionMap.mockRejectedValue(new Error("nope"));
    api.listDeclarations.mockResolvedValue({ items: [decl({ can_run_tests: "maybe", network: "satellite" })], next_cursor: null });
    render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    const list = await screen.findByTestId("declarations-list");
    expect(list).toHaveTextContent("sess-111");
    expect(list).toHaveTextContent("maybe");
    expect(list).toHaveTextContent("satellite");
  });

  it("says absence means unknown", async () => {
    api.listDeclarations.mockResolvedValue({ items: [], next_cursor: null });
    render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByText("collab_ops.declarations.empty")).toBeInTheDocument();
    expect(screen.getByTestId("declarations-claims-note")).toHaveTextContent("not_permissions");
  });

  it("shows a load error", async () => {
    api.listDeclarations.mockRejectedValue(new ApiError("boom", 500));
    render(DeclarationsCard, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
  });
});
