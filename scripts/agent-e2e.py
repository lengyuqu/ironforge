#!/usr/bin/env python3
"""Real-run check of the agent write path: a branch-limited token, the real
`ironforge-mcp`, a real server and a real CI run.

Starts `ironforge serve --allow-host-execution` on a throw-away SQLite database
and drives it through MCP tools only (as an agent would):

  commit_file (dry run, then for real) -> a push-style pipeline appears for the
  commit -> wait_pipeline sees it succeed; a second branch whose CI script fails
  -> wait_pipeline reports failed with a summary; create_pr (dry run, real,
  retried with the same idempotency key) -> get_pr_context / export_pr;
  wait_repo_events sees the pushes; a write to `main` is refused for the limited
  token.

Host execution of CI jobs is a security-sensitive opt-in and is used here only
because the instance is throw-away and the job scripts are written by this
script. Not run in CI. Single machine, SQLite, no Docker.

Usage: cargo build -p rg-cli -p rg-mcp && python3 scripts/agent-e2e.py [--keep]
"""

import argparse
import importlib.util
import os
import shutil
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("collab_e2e", os.path.join(HERE, "collab-e2e.py"))
e2e = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(e2e)

say, check, http = e2e.say, e2e.check, e2e.http
OWNER = "agent-e2e"
REPO = "work"
CI_OK = "stages:\n  - test\nunit:\n  stage: test\n  script:\n    - echo all good\n"
CI_FAIL = "stages:\n  - test\nunit:\n  stage: test\n  script:\n    - echo boom-marker >&2\n    - exit 3\n"


def wait_until(describe, fn, tries=100, delay=0.2):
    for _ in range(tries):
        value = fn()
        if value:
            return value
        time.sleep(delay)
    check(False, f"timed out waiting for: {describe}")


def run(work):
    server = e2e.Server(work, extra_args=["--allow-host-execution"])
    mcp = None
    try:
        server.start()
        base = server.base
        say("server started", f"{base} (sqlite, host-executed CI)")

        st, reg = http(base, "POST", "/api/v1/users/register",
                       body={"username": OWNER, "email": f"{OWNER}@example.com", "password": e2e.PASSWORD})
        check(st in (200, 201), f"register: {st} {reg}")
        jwt = reg["token"]
        st, repo = http(base, "POST", "/api/v1/repos", jwt,
                        body={"name": REPO, "auto_init": True, "is_private": False})
        check(st == 201, f"create repo: {st} {repo}")
        api = f"/api/v1/repos/{OWNER}/{REPO}"

        # The CI config goes onto main with the owner's login (the agent cannot write main).
        st, v = http(base, "POST", f"{api}/contents/.ironforge-ci.yml", jwt,
                     body={"branch": "main", "message": "add CI", "content": CI_OK})
        check(st == 200, f"commit CI config: {st} {v}")

        st, tok = http(base, "POST", "/api/v1/users/tokens", jwt,
                       body={"name": "agent", "scopes": "repo:write",
                             "repos": [f"{OWNER}/{REPO}"], "refs": ["agent/*"]})
        check(st in (200, 201), f"create limited token: {st} {tok}")
        mcp = e2e.McpClient(base, tok["token"], os.path.join(work, "no-session"), work, "agent")
        say("agent connected", "token limited to this repository and agent/*")
        coords = {"owner": OWNER, "repo": REPO}

        # 1. A dry run writes nothing; the real commit creates the branch.
        dry = mcp.ok("commit_file", **coords, path="notes.txt", content="hello\n",
                     message="add notes", branch="agent/pass", dry_run=True)
        check(dry.get("dry_run") is True, f"dry run answer: {dry}")
        st, branches = http(base, "GET", f"{api}/branches", jwt)
        check("agent/pass" not in str(branches), "dry run must not create the branch")
        done = mcp.ok("commit_file", **coords, path="notes.txt", content="hello\n",
                      message="add notes", branch="agent/pass", idempotency_key="e2e-pass-1")
        sha_pass = done["commit_sha"]
        check(len(sha_pass) == 40, f"commit sha: {done}")
        again = mcp.ok("commit_file", **coords, path="notes.txt", content="hello\n",
                       message="add notes", branch="agent/pass", idempotency_key="e2e-pass-1")
        check(again["commit_sha"] == sha_pass, "a retried commit with the same key must replay")
        say("commit_file", f"dry run wrote nothing; real commit {sha_pass[:12]}; retry replayed")

        # 2. The commit has a pipeline, and wait_pipeline sees it finish.
        def find_pipeline(sha):
            listing = mcp.ok("list_pipelines", **coords)
            rows = listing.get("data", listing) if isinstance(listing, dict) else listing
            return next((p for p in rows if p.get("commit_sha") == sha), None)

        pipeline = wait_until("a pipeline for the committed sha", lambda: find_pipeline(sha_pass))
        result = None
        for _ in range(6):
            result = mcp.ok("wait_pipeline", **coords, pipeline_id=pipeline["id"], timeout=20)
            if result.get("terminal"):
                break
        check(result and result.get("terminal"), f"pipeline never finished: {result}")
        check(result.get("status") == "success" or result.get("pipeline", {}).get("status") == "success",
              f"expected a successful pipeline: {result}")
        say("wait_pipeline", f"pipeline {pipeline['id']} finished: success")

        # 3. A failing script is reported as failed with its summary.
        st, cfg = http(base, "GET", f"{api}/blob/.ironforge-ci.yml?ref=main", jwt)
        check(st == 200, f"read CI config: {st}")
        blob = cfg.get("sha")
        check(blob, f"config blob sha: {cfg}")
        fail = mcp.ok("commit_file", **coords, path=".ironforge-ci.yml", content=CI_FAIL,
                      message="break CI", branch="agent/fail", sha=blob)
        sha_fail = fail["commit_sha"]
        pipeline_fail = wait_until("a pipeline for the failing commit", lambda: find_pipeline(sha_fail))
        for _ in range(6):
            result = mcp.ok("wait_pipeline", **coords, pipeline_id=pipeline_fail["id"], timeout=20)
            if result.get("terminal"):
                break
        text = str(result)
        check(result.get("terminal"), f"failing pipeline never finished: {result}")
        check("failed" in text, f"expected a failed pipeline: {result}")
        say("wait_pipeline", f"failing branch reported failed; summary present: {'boom-marker' in text}")

        # 4. A pull request: dry run, real, retried with the same key.
        plan = mcp.ok("create_pr", **coords, title="Add notes", head="agent/pass", base="main", dry_run=True)
        check(plan["would"][0]["number"] == 1 and plan["would"][0]["head_sha"] == sha_pass, f"plan: {plan}")
        check(plan["existing_open_pull_requests"] == [], "no pull request yet")
        pr = mcp.ok("create_pr", **coords, title="Add notes", head="agent/pass", base="main",
                    idempotency_key="e2e-pr-1")
        retry = mcp.ok("create_pr", **coords, title="Add notes", head="agent/pass", base="main",
                       idempotency_key="e2e-pr-1")
        check(pr["number"] == 1 and retry["number"] == 1, f"one pull request expected: {pr} / {retry}")
        st, listing = http(base, "GET", f"{api}/pulls", jwt)
        rows = listing.get("data", listing)
        check(len(rows) == 1, f"the retry must not open a second pull request: {listing}")
        say("create_pr", "dry run planned #1; real call and keyed retry gave one pull request")

        ctx = mcp.ok("get_pr_context", **coords, number=1)
        check(ctx["pull_request"]["head_sha"] == sha_pass, f"context head: {ctx['pull_request']}")
        check(ctx["ci"][0]["pipeline"]["status"] == "success" if "ci" in ctx and isinstance(ctx["ci"], list)
              else True, f"context ci: {ctx.get('ci')}")
        export = mcp.ok("export_pr", **coords, number=1)
        check(export["anchors"]["head_ref"] == "refs/pull/1/head", f"anchors: {export['anchors']}")
        check(export["anchors"]["head_ref_sha"] == sha_pass, "refs/pull/1/head must point at the head")
        check(export["ci"]["pipeline"]["status"] == "success", f"export ci: {export['ci']}")
        say("get_pr_context / export_pr", "head, CI result and refs/pull/1/head agree")

        # 5. The pushes reached the event log.
        events = mcp.ok("wait_repo_events", **coords, since=0, types="push", timeout=5)
        shas = {e["payload"].get("after") for e in events["events"]}
        check(sha_pass in shas and sha_fail in shas, f"push events missing: {shas}")
        say("wait_repo_events", f"{len(events['events'])} push events, both commits present")

        # 6. The limited token cannot write main.
        is_error, value = mcp.tool("commit_file", **coords, path="x.txt", content="x\n",
                                   message="sneak", branch="main")
        check(is_error and "403" in str(value), f"a write to main must be refused: {value}")
        say("limit holds", "commit_file to main refused with 403")
        print("OK: agent write path verified end to end")
    finally:
        if mcp:
            try:
                mcp.close()
            except Exception:
                pass
        server.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    args = parser.parse_args()
    for binary in (e2e.SERVER_BIN, e2e.MCP_BIN):
        check(os.access(binary, os.X_OK), f"missing binary {binary}; run: cargo build -p rg-cli -p rg-mcp")
    work = tempfile.mkdtemp(prefix="ironforge-agent-e2e.")
    status = 1
    try:
        run(work)
        status = 0
    finally:
        if args.keep or status != 0:
            print(f"work directory kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)
    sys.exit(status)


if __name__ == "__main__":
    main()
