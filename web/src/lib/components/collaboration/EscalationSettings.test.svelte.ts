import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { ApiError } from "$lib/api/_base.svelte";

vi.mock("$lib/components/toast.svelte", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("$lib/i18n", () => {
  const t = (key: string, params?: Record<string, unknown>) =>
    params && typeof params === "object" ? `${key} ${JSON.stringify(params)}` : key;
  return { createT: () => t, t };
});

const api = vi.hoisted(() => ({ getEscalation: vi.fn(), putEscalation: vi.fn() }));
vi.mock("$lib/api/collabOps", async (orig) => ({ ...(await orig<typeof import("$lib/api/collabOps")>()), collabOps: api }));

import EscalationSettings from "./EscalationSettings.svelte";
import type { EscalationSettings as Settings } from "$lib/api/collabOps";

function settings(over: Partial<Settings> = {}): Settings {
  return {
    enabled: false,
    blocked_after_secs: 14400,
    question_after_secs: 3600,
    handoff_after_secs: 900,
    repeat_after_secs: 0,
    quiet_hours: null,
    timezone: "UTC",
    version: 0,
    source: "default",
    updated_at: null,
    updated_by_user_id: null,
    ...over,
  };
}

const field = (key: string) => screen.getByLabelText(key) as HTMLInputElement;

describe("EscalationSettings.svelte", () => {
  beforeEach(() => {
    api.getEscalation.mockReset().mockResolvedValue(settings());
    api.putEscalation.mockReset();
  });

  it("shows defaults as 'nothing saved, off' and explains what is and is not notified", async () => {
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    const view = await screen.findByTestId("escalation-view");
    expect(view).toHaveTextContent("collab_ops.escalation.default_source");
    expect(view).toHaveTextContent("collab_ops.escalation.off");
    expect(screen.getByText("collab_ops.escalation.what_3")).toBeInTheDocument();
    expect(screen.getByText("collab_ops.escalation.what_4")).toBeInTheDocument();
  });

  it("saves minutes as seconds with the version it read", async () => {
    api.getEscalation.mockResolvedValue(settings({ version: 4, source: "stored", updated_at: "2026-11-06T00:00:00Z", updated_by_user_id: 1 }));
    api.putEscalation.mockResolvedValue(settings({ version: 5, enabled: true, source: "stored", blocked_after_secs: 1800 }));
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.escalation.edit" }));
    await fireEvent.click(screen.getByLabelText("collab_ops.escalation.enabled"));
    await fireEvent.input(field("collab_ops.escalation.blocked_after"), { target: { value: "30" } });
    await fireEvent.click(screen.getByLabelText("collab_ops.escalation.quiet"));
    await fireEvent.input(field("collab_ops.escalation.quiet_start"), { target: { value: "23:30" } });
    await fireEvent.input(field("collab_ops.escalation.quiet_end"), { target: { value: "06:00" } });
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.escalation.save" }));
    await waitFor(() => expect(api.putEscalation).toHaveBeenCalledTimes(1));
    expect(api.putEscalation.mock.calls[0].slice(0, 2)).toEqual(["alice", "demo"]);
    expect(api.putEscalation.mock.calls[0][2]).toEqual({
      expected_version: 4,
      enabled: true,
      blocked_after_secs: 1800,
      question_after_secs: 3600,
      handoff_after_secs: 900,
      repeat_after_secs: 0,
      quiet_hours: { start_minute: 1410, end_minute: 360 },
    });
  });

  it("does not call the server for an out-of-range value and says why", async () => {
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.escalation.edit" }));
    await fireEvent.input(field("collab_ops.escalation.question_after"), { target: { value: "0" } });
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.escalation.save" }));
    expect(await screen.findByTestId("escalation-problem")).toHaveTextContent("threshold_range");
    expect(api.putEscalation).not.toHaveBeenCalled();
  });

  it("on VERSION_CONFLICT shows the stored values again and says the edit was not applied", async () => {
    api.putEscalation.mockRejectedValue(new ApiError("settings changed", 409, "VERSION_CONFLICT"));
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.escalation.edit" }));
    api.getEscalation.mockResolvedValue(settings({ version: 7, source: "stored", enabled: true, updated_at: "2026-11-06T00:00:00Z", updated_by_user_id: 2 }));
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.escalation.save" }));
    expect(await screen.findByTestId("escalation-conflict")).toHaveTextContent("collab_ops.escalation.conflict_reloaded");
    expect(screen.getByText(/settings changed/)).toBeInTheDocument();
    expect(screen.getByTestId("escalation-view")).toHaveTextContent("collab_ops.escalation.on");
    expect(api.getEscalation).toHaveBeenCalledTimes(2);
  });

  it("tells a non-admin they are read-only after the server refuses the write", async () => {
    api.putEscalation.mockRejectedValue(new ApiError("repository admin access required", 403));
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    await fireEvent.click(await screen.findByRole("button", { name: "collab_ops.escalation.edit" }));
    await fireEvent.click(screen.getByRole("button", { name: "collab_ops.escalation.save" }));
    expect(await screen.findByTestId("escalation-not-admin")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "collab_ops.escalation.edit" })).toBeDisabled();
  });

  it("disables editing up front when another admin-only read already said the viewer is not an admin", async () => {
    render(EscalationSettings, { props: { owner: "alice", repo: "demo", knownNotAdmin: true } });
    expect(await screen.findByTestId("escalation-not-admin")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "collab_ops.escalation.edit" })).toBeDisabled();
    expect(api.putEscalation).not.toHaveBeenCalled();
  });

  it("shows a load error", async () => {
    api.getEscalation.mockRejectedValue(new ApiError("boom", 500));
    render(EscalationSettings, { props: { owner: "alice", repo: "demo" } });
    expect(await screen.findByText(/boom/)).toBeInTheDocument();
  });
});
