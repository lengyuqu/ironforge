#!/usr/bin/env python3
"""Real-run check of three things `agent-e2e.py` does not cover: the branch
limit on `delete_branch`, the remote MCP transport (`ironforge-mcp --http`) and
fork pull requests.

Starts `ironforge serve` on a throw-away SQLite database and drives it over
plain HTTP and over a real `ironforge-mcp --http` process:

  1. branch-limited token: `delete_branch` inside the limit succeeds, outside it
     is 403, `branches/protection` is 403, the default branch / a protected
     branch / a branch with an open pull request are 409 with the documented
     `reason`, a stale `expected_sha` is 409 `REF_ADVANCED`, and `dry_run`
     deletes nothing;
  2. remote MCP: `initialize` -> `tools/list` -> `tools/call` for `commit_file`,
     `wait_pipeline` and `delete_branch`; no token is refused with 401; two
     tokens do not see each other's sessions; a second `initialize` opens a new
     session that works;
  3. fork pull request: a second user forks, commits in the fork, opens a pull
     request against the original; `export_pr` anchors follow the fork's branch
     and `base_sha_when_opened` does not move when the base branch advances.

Host execution of CI is a security-sensitive opt-in and is used here only
because the instance is throw-away. Not run in CI. Single machine, SQLite, no
Docker.

Usage: cargo build -p rg-cli -p rg-mcp && python3 scripts/agent-e2e-remote.py [--keep]
"""

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
_spec = importlib.util.spec_from_file_location("collab_e2e", os.path.join(HERE, "collab-e2e.py"))
e2e = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(e2e)

say, check, http = e2e.say, e2e.check, e2e.http
SERVER_BIN, MCP_BIN = e2e.SERVER_BIN, e2e.MCP_BIN
PASSWORD = e2e.PASSWORD

ALICE = "remote-alice"
BOB = "remote-bob"
REPO = "work"
CI_OK = "stages:\n  - test\nunit:\n  stage: test\n  script:\n    - echo all good\n"

# ── remote MCP client ───────────────────────────────────────────────────


class RemoteMcp:
    """A minimal streamable-HTTP MCP client: one JSON-RPC message per POST."""

    def __init__(self, url, token=None, session=None):
        self.url = url
        self.token = token
        self.session = session
        self.next_id = 1

    def _post(self, payload):
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if self.session:
            headers["Mcp-Session-Id"] = self.session
        request = urllib.request.Request(
            self.url, data=json.dumps(payload).encode(), headers=headers, method="POST"
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read().decode()
                return response.status, dict(response.headers), (json.loads(raw) if raw else None)
        except urllib.error.HTTPError as error:
            raw = error.read().decode()
            try:
                return error.code, dict(error.headers), (json.loads(raw) if raw else None)
            except json.JSONDecodeError:
                return error.code, dict(error.headers), raw

    def initialize(self):
        status, headers, body = self._post(
            {
                "jsonrpc": "2.0",
                "id": self.next_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "agent-e2e-remote", "version": "0"},
                },
            }
        )
        self.next_id += 1
        session = next(
            (v for k, v in headers.items() if k.lower() == "mcp-session-id"), None
        )
        return status, session, body

    def rpc(self, method, params=None):
        status, _headers, body = self._post(
            {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}
        )
        self.next_id += 1
        return status, body

    def tool(self, name, **args):
        """Call a tool; returns (is_error, parsed JSON or raw text)."""
        status, body = self.rpc("tools/call", {"name": name, "arguments": args})
        check(status == 200 and isinstance(body, dict) and "result" in body, f"{name}: {status} {body}")
        result = body["result"]
        text = result["content"][0]["text"]
        try:
            value = json.loads(text)
        except json.JSONDecodeError:
            value = text
        return bool(result.get("isError")), value

    def ok(self, name, **args):
        is_error, value = self.tool(name, **args)
        check(not is_error, f"MCP {name} failed: {value}")
        return value


class McpServer:
    """`ironforge-mcp --http 127.0.0.1:<port>` next to a running server."""

    def __init__(self, base, work, port):
        self.work = work
        self.port = port
        self.url = f"http://127.0.0.1:{port}/mcp"
        env = dict(os.environ, IRONFORGE_URL=base, RUST_LOG="info")
        env.pop("IRONFORGE_PAT", None)
        self.stderr = open(os.path.join(work, "mcp-http.stderr"), "wb")
        self.proc = subprocess.Popen(
            [MCP_BIN, "--http", f"127.0.0.1:{port}"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=self.stderr,
            env=env,
        )
        for _ in range(100):
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}/", timeout=1):
                    break
            except urllib.error.HTTPError:
                break  # the server answers (with an error) -> it is up
            except OSError:
                time.sleep(0.1)

    def stop(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.stderr.close()


def wait_until(describe, fn, tries=100, delay=0.2):
    for _ in range(tries):
        value = fn()
        if value:
            return value
        time.sleep(delay)
    check(False, f"timed out waiting for: {describe}")


# ── the scenarios ───────────────────────────────────────────────────────


def register(base, name):
    st, body = http(
        base,
        "POST",
        "/api/v1/users/register",
        body={"username": name, "email": f"{name}@example.com", "password": PASSWORD},
    )
    check(st in (200, 201), f"register {name}: {st} {body}")
    return body["token"]


def delete_branch(base, token, owner, repo, branch, expected_sha=None, dry_run=False):
    query = []
    if expected_sha:
        query.append(f"expected_sha={expected_sha}")
    if dry_run:
        query.append("dry_run=true")
    path = f"/api/v1/repos/{owner}/{repo}/branches/{branch}"
    if query:
        path += "?" + "&".join(query)
    return http(base, "DELETE", path, token)


def run(work):
    server = e2e.Server(work, extra_args=["--allow-host-execution"])
    mcp_server = None
    try:
        server.start()
        base = server.base
        say("server started", f"{base} (sqlite, host-executed CI)")

        alice = register(base, ALICE)
        bob = register(base, BOB)
        st, repo = http(
            base, "POST", "/api/v1/repos", alice,
            body={"name": REPO, "auto_init": True, "is_private": False},
        )
        check(st == 201, f"create repo: {st} {repo}")
        api = f"/api/v1/repos/{ALICE}/{REPO}"
        st, _ = http(
            base, "POST", f"{api}/contents/.ironforge-ci.yml", alice,
            body={"branch": "main", "message": "add CI", "content": CI_OK},
        )
        check(st == 200, "commit CI config")

        # A branch-limited token: this repository only, agent/* only.
        st, tok = http(
            base, "POST", "/api/v1/users/tokens", alice,
            body={"name": "agent", "scopes": "repo:write",
                  "repos": [f"{ALICE}/{REPO}"], "refs": ["agent/*"]},
        )
        check(st in (200, 201), f"create limited token: {st} {tok}")
        agent = tok["token"]

        def commit(token, branch, path, message="add"):
            st, body = http(
                base, "POST", f"{api}/contents/{path}", token,
                body={"branch": branch, "message": message, "content": "x\n"},
            )
            check(st == 200, f"commit {path} on {branch}: {st} {body}")
            return body["commit_sha"]

        # ── 1. the branch limit on delete_branch ────────────────────────
        commit(agent, "agent/keep", "a.txt")
        # A protected branch and a branch outside the token's limit both need
        # the owner's login to create (the token may only write agent/*).
        commit(alice, "release/1.0", "r.txt")
        commit(alice, "feature/outside", "f.txt")
        sha_agent = commit(agent, "agent/target", "c.txt")

        # Inside the limit: dry run deletes nothing, the real call deletes.
        st, body = delete_branch(base, agent, ALICE, REPO, "agent/target", dry_run=True)
        check(st == 200 and body.get("dry_run") is True, f"dry run: {st} {body}")
        st, listing = http(base, "GET", f"{api}/branches", alice)
        check("agent/target" in str(listing), "dry run must not delete the branch")
        st, body = delete_branch(base, agent, ALICE, REPO, "agent/target", expected_sha=sha_agent)
        check(st == 200, f"delete inside the limit: {st} {body}")
        st, listing = http(base, "GET", f"{api}/branches", alice)
        check("agent/target" not in str(listing), "the branch must be gone")
        say("delete_branch", "inside the limit: dry run kept it, the real call removed it")

        # Outside the limit, and a branch that does not exist.
        st, body = delete_branch(base, agent, ALICE, REPO, "feature/outside")
        check(st == 403, f"outside the ref limit must be 403: {st} {body}")
        st, body = http(base, "POST", f"{api}/branches/protection", agent,
                        body={"branch_name": "release/1.0"})
        check(st == 403, f"branches/protection must be 403 for a limited token: {st} {body}")
        say("delete_branch", "outside the limit and the protection path are both 403")

        # The owner's login can create a protection rule; a protected branch
        # then refuses deletion.
        st, body = http(base, "POST", f"{api}/branches/protection", alice,
                        body={"branch_name": "release/1.0"})
        check(st in (200, 201), f"create protection: {st} {body}")
        st, body = delete_branch(base, alice, ALICE, REPO, "release/1.0")
        check(st == 409 and body.get("error", {}).get("reason") == "BRANCH_PROTECTED",
              f"protected branch must be 409 BRANCH_PROTECTED: {st} {body}")
        st, body = delete_branch(base, alice, ALICE, REPO, "main")
        reason = body.get("error", {}).get("reason")
        check(st == 409 and reason == "DEFAULT_BRANCH",
              f"default branch must be 409 DEFAULT_BRANCH: {st} {body}")

        # A branch with an open pull request is refused, and the reason names
        # the pull request.
        commit(agent, "agent/busy", "d.txt")
        st, pr = http(base, "POST", f"{api}/pulls", alice,
                      body={"title": "busy", "head": "agent/busy", "base": "main"})
        check(st == 201, f"open pull request: {st} {pr}")
        st, body = delete_branch(base, agent, ALICE, REPO, "agent/busy")
        error = body.get("error", {})
        check(st == 409 and error.get("reason") == "OPEN_PULL_REQUESTS",
              f"open pull request must be 409 OPEN_PULL_REQUESTS: {st} {body}")
        listed = error.get("details", {}).get("pull_requests") or []
        check([row.get("number") for row in listed] == [pr["number"]],
              f"the refusal must name the pull request: {body}")

        # A stale expected_sha is refused instead of deleting blindly.
        commit(agent, "agent/moved", "e.txt")
        st, body = delete_branch(base, agent, ALICE, REPO, "agent/moved", expected_sha="0" * 40)
        check(st == 409 and body.get("error", {}).get("reason") == "REF_ADVANCED",
              f"stale expected_sha must be 409 REF_ADVANCED: {st} {body}")
        say("delete_branch", "default/protected/open-PR/stale-sha all refused with the right reason")

        # ── 2. the remote MCP transport ─────────────────────────────────
        port = e2e.free_port()
        mcp_server = McpServer(base, work, port)
        say("remote MCP started", f"POST {mcp_server.url}")

        anonymous = RemoteMcp(mcp_server.url)
        status, session, _body = anonymous.initialize()
        check(status == 401 and not session, f"no token must be refused: {status} {session}")

        client = RemoteMcp(mcp_server.url, agent)
        status, session, body = client.initialize()
        check(status == 200 and session, f"initialize: {status} {session} {body}")
        client.session = session
        status, body = client.rpc("tools/list")
        check(status == 200, f"tools/list: {status} {body}")
        names = {tool["name"] for tool in body["result"]["tools"]}
        for needed in ("commit_file", "wait_pipeline", "delete_branch", "export_pr"):
            check(needed in names, f"tools/list must offer {needed}")
        say("remote MCP", f"initialize + tools/list ok ({len(names)} tools)")

        coords = {"owner": ALICE, "repo": REPO}
        committed = client.ok(
            "commit_file", **coords, path="remote.txt", content="over http\n",
            message="remote commit", branch="agent/remote",
        )
        sha = committed["commit_sha"]
        pipeline = wait_until(
            "a pipeline for the commit made over the remote MCP transport",
            lambda: next(
                (p for p in client.ok("list_pipelines", **coords).get("data", [])
                 if p.get("commit_sha") == sha),
                None,
            ),
        )
        result = None
        for _ in range(6):
            result = client.ok("wait_pipeline", **coords, pipeline_id=pipeline["id"], timeout=20)
            if result.get("terminal"):
                break
        check(result and result.get("terminal"), f"pipeline never finished: {result}")
        client.ok("delete_branch", **coords, branch="agent/remote")
        st, listing = http(base, "GET", f"{api}/branches", alice)
        check("agent/remote" not in str(listing), "the branch deleted over the remote transport must be gone")
        say("remote MCP", "commit_file -> wait_pipeline -> delete_branch all work over HTTP")

        # Two tokens: sessions are bound to the token that opened them.
        status, other_session, _ = RemoteMcp(mcp_server.url, alice).initialize()
        check(status == 200 and other_session and other_session != session,
              "a different token must get its own session")
        intruder = RemoteMcp(mcp_server.url, alice, session=session)
        status, body = intruder.rpc("tools/list")
        check(status == 404, f"a session must not be usable with another token: {status} {body}")

        # Reconnect: a fresh initialize works and the old session is still fine.
        reconnected = RemoteMcp(mcp_server.url, agent)
        status, second_session, _ = reconnected.initialize()
        check(status == 200 and second_session and second_session != session, "reconnect")
        reconnected.session = second_session
        status, _ = reconnected.rpc("tools/list")
        check(status == 200, "the new session must work")
        status, _ = client.rpc("tools/list")
        check(status == 200, "the first session must keep working")
        say("remote MCP", "sessions are per token; a reconnect opens a new one without breaking the old")

        # ── 3. a fork pull request ──────────────────────────────────────
        st, fork = http(base, "POST", f"{api}/fork", bob, body={})
        check(st in (200, 201, 202), f"fork: {st} {fork}")
        fork_api = f"/api/v1/repos/{BOB}/{REPO}"
        # A fork may be created asynchronously (202): wait for it to be usable.
        wait_until(
            "the fork to become usable",
            lambda: http(base, "GET", f"{fork_api}/branches", bob)[0] == 200,
            tries=100,
        )

        st, body = http(base, "POST", f"{fork_api}/contents/fork.txt", bob,
                        body={"branch": "feat", "message": "in the fork", "content": "x\n"})
        check(st == 200, f"commit in the fork: {st} {body}")
        fork_sha = body["commit_sha"]
        st, pr = http(base, "POST", f"{api}/pulls", bob,
                      body={"title": "from a fork", "head": f"{BOB}:feat", "base": "main"})
        check(st == 201, f"fork pull request: {st} {pr}")
        number = pr["number"]

        def export():
            st, body = http(base, "GET", f"{api}/pulls/{number}/export", alice)
            check(st == 200, f"export: {st} {body}")
            return body

        def anchors():
            return export()["anchors"]

        def head_ref_sha():
            value = anchors()
            return value["head_ref_sha"]

        wait_until("refs/pull/<n>/head for the fork pull request",
                   lambda: head_ref_sha() == fork_sha)
        first = anchors()
        check(first["head_ref"] == f"refs/pull/{number}/head", f"anchors: {first}")
        base_when_opened = first["base_sha_when_opened"]
        check(base_when_opened, f"the base at open must be recorded: {first}")
        base_tip_at_open = first["base_tip_sha"]
        say("fork PR", f"#{number} anchored at refs/pull/{number}/head -> {fork_sha[:12]}")

        # The fork branch moves; the anchor follows.
        st, body = http(base, "POST", f"{fork_api}/contents/fork2.txt", bob,
                        body={"branch": "feat", "message": "more", "content": "y\n"})
        check(st == 200, f"second commit in the fork: {st} {body}")
        moved = body["commit_sha"]
        wait_until("the anchor to follow the fork branch", lambda: head_ref_sha() == moved)

        # The base branch advances; the recorded base at open does not move.
        st, body = http(base, "POST", f"{api}/contents/base.txt", alice,
                        body={"branch": "main", "message": "base moves", "content": "z\n"})
        check(st == 200, f"advance base: {st} {body}")
        after = anchors()
        check(after["base_sha_when_opened"] == base_when_opened,
              f"base_sha_when_opened moved: {after}")
        check(after["base_tip_sha"] == body["commit_sha"],
              f"base_tip_sha must be the new tip: {after}")
        check(after["base_tip_sha"] != base_tip_at_open, "the base tip must have moved")
        say("fork PR", "anchor follows the fork; base_sha_when_opened stays put while the base moves")

        print("OK: branch limit, remote MCP transport and fork pull requests verified")
    finally:
        if mcp_server:
            mcp_server.stop()
        server.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    args = parser.parse_args()
    for binary in (SERVER_BIN, MCP_BIN):
        check(os.access(binary, os.X_OK),
              f"missing binary {binary}; run: cargo build -p rg-cli -p rg-mcp")
    work = tempfile.mkdtemp(prefix="ironforge-agent-e2e-remote.")
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
