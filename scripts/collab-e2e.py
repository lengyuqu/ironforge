#!/usr/bin/env python3
"""T12: two different clients, one account, one real Git repository.

Starts the real `ironforge` server on a throw-away SQLite database and drives
the collaboration flow of ironforge-docs/architecture/multi-agent-collaboration-design-2026-11.md
through two unrelated clients:

  * client A: the real `ironforge-mcp` binary over stdio (what an MCP harness
    such as Claude Code would run), authenticating with a repo-scoped PAT;
  * client B: a plain HTTP client (stdlib urllib), authenticating with the
    account's login JWT and holding its session secret in its own storage.

Nothing is mocked. The script restarts the MCP process (client crash) and the
server (service restart) mid-flow and checks that state and read positions
survive. It exits non-zero on the first failed assertion and prints a transcript.

Usage:
  cargo build -p rg-cli -p rg-mcp
  python3 scripts/collab-e2e.py [--scenario t12|p0|x04|ci-compat] [--multi-instance] [--keep] [--json-out FILE]

Environment: IRONFORGE_BIN (default target/debug/ironforge),
             IRONFORGE_MCP_BIN (default target/debug/ironforge-mcp),
             IRONFORGE_E2E_DB_URL (X04/ci-compat only; MUST be a new disposable database).
             The script applies migrations and creates users and repositories.
             --multi-instance starts two processes sharing that database and a
             local temporary Git root; it does not simulate separate hosts.
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SERVER_BIN = os.environ.get("IRONFORGE_BIN", os.path.join(ROOT, "target/debug/ironforge"))
MCP_BIN = os.environ.get("IRONFORGE_MCP_BIN", os.path.join(ROOT, "target/debug/ironforge-mcp"))
JWT_SECRET = "collab-e2e-jwt-secret-2026"
OWNER = "collab-e2e"
REPO = "pair-work"
PASSWORD = "Qz7$wRtm"  # test-only credential for the throw-away instance

transcript = []
secrets_seen = []  # every session secret handed out; must never appear in logs/outputs


def say(step, detail=""):
    line = f"[{len(transcript) + 1:02d}] {step}" + (f" — {detail}" if detail else "")
    transcript.append(line)
    print(line, flush=True)


def check(condition, message):
    if not condition:
        print(f"ASSERTION FAILED: {message}", file=sys.stderr)
        raise SystemExit(1)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


# ── server ────────────────────────────────────────────────────────────

class Server:
    def __init__(self, work, db_url=None, label="server", extra_args=None):
        self.work = work
        self.db_url = db_url or f"sqlite://{work}/ironforge.db?mode=rwc"
        self.label = label
        self.extra_args = extra_args or []
        self.http_port = free_port()
        self.ssh_port = free_port()
        self.base = f"http://127.0.0.1:{self.http_port}"
        self.proc = None
        self.log = open(os.path.join(work, f"{label}.log"), "ab")

    def start(self):
        os.makedirs(os.path.join(self.work, "repos"), exist_ok=True)
        self.proc = subprocess.Popen(
            [
                SERVER_BIN, "serve",
                "--repo-root", os.path.join(self.work, "repos"),
                "--http-addr", f"127.0.0.1:{self.http_port}",
                "--ssh-addr", f"127.0.0.1:{self.ssh_port}",
                "--host-key", os.path.join(self.work, f"{self.label}-host-key"),
                "--db-url", self.db_url,
                "--jwt-secret", JWT_SECRET,
            ] + self.extra_args,
            stdout=self.log, stderr=self.log,
            # Verbose on purpose: the secret-leak scan below is only meaningful
            # if the server actually logs request activity.
            env=dict(os.environ, RUST_LOG="debug"),
        )
        for _ in range(200):
            try:
                urllib.request.urlopen(self.base + "/health", timeout=1).read()
                return
            except Exception:
                check(self.proc.poll() is None, "server exited before becoming healthy")
                time.sleep(0.1)
        check(False, "server did not become healthy")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()

    def log_text(self):
        self.log.flush()
        with open(os.path.join(self.work, f"{self.label}.log"), "rb") as f:
            return f.read().decode("utf-8", "replace")


# ── client B: plain HTTP ──────────────────────────────────────────────

def http(base, method, path, token=None, session=None, body=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if session:
        headers["X-Ironforge-Session"] = session["id"]
        headers["X-Ironforge-Session-Secret"] = session["secret"]
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as err:
        raw = err.read().decode()
        try:
            return err.code, json.loads(raw)
        except ValueError:
            return err.code, raw


# ── client A: the real MCP binary over stdio ──────────────────────────

class McpClient:
    def __init__(self, base, pat, session_file, work, label):
        env = dict(os.environ, IRONFORGE_URL=base, IRONFORGE_PAT=pat,
                   IRONFORGE_COLLAB_SESSION_FILE=session_file, RUST_LOG="trace")
        self.stderr_path = os.path.join(work, f"mcp-{label}.stderr")
        self.stderr = open(self.stderr_path, "wb")
        self.proc = subprocess.Popen([MCP_BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=self.stderr, env=env)
        self.next_id = 1
        resp = self.rpc("initialize", {})
        check(resp["result"]["serverInfo"]["name"] == "ironforge-mcp", "MCP initialize")
        self.server_info = resp["result"]["serverInfo"]

    def rpc(self, method, params):
        req = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params}
        self.next_id += 1
        self.proc.stdin.write((json.dumps(req) + "\n").encode())
        self.proc.stdin.flush()
        line = self.proc.stdout.readline().decode()
        check(line, "MCP process closed its output")
        return json.loads(line)

    def tool(self, name, **args):
        """Call a tool; returns (is_error, parsed JSON or raw text)."""
        args.setdefault("owner", OWNER)
        args.setdefault("repo", REPO)
        resp = self.rpc("tools/call", {"name": name, "arguments": args})
        result = resp["result"]
        text = result["content"][0]["text"]
        try:
            value = json.loads(text)
        except ValueError:
            value = text
        return bool(result.get("isError")), value

    def ok(self, name, **args):
        is_error, value = self.tool(name, **args)
        check(not is_error, f"MCP {name} failed: {value}")
        return value

    def crash(self):
        self.proc.kill()
        self.proc.wait()
        self.stderr.close()

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)
        self.stderr.close()

    def stderr_text(self):
        with open(self.stderr_path, "rb") as f:
            return f.read().decode("utf-8", "replace")


# ── git ───────────────────────────────────────────────────────────────

def git(args, cwd, token=None):
    # Identity and config come from the environment, never from user git config.
    env = dict(os.environ, GIT_AUTHOR_NAME="e2e", GIT_AUTHOR_EMAIL="e2e@example.invalid",
               GIT_COMMITTER_NAME="e2e", GIT_COMMITTER_EMAIL="e2e@example.invalid",
               GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null", GIT_TERMINAL_PROMPT="0")
    prefix = ["git"]
    if token:
        prefix += ["-c", f"http.extraHeader=Authorization: Bearer {token}"]
    out = subprocess.run(prefix + args, cwd=cwd, env=env, capture_output=True, text=True)
    check(out.returncode == 0, f"git {args[0]} failed: {out.stderr}")
    return out.stdout.strip()


# ── the scenario ──────────────────────────────────────────────────────

def run(work, keep):
    server = Server(work)
    mcp = None
    try:
        server.start()
        base = server.base
        say("server started", f"{base} (sqlite, {platform.system()} {platform.machine()})")

        # Account, two kinds of credential for the same account.
        st, reg = http(base, "POST", "/api/v1/users/register",
                       body={"username": OWNER, "email": f"{OWNER}@example.com", "password": PASSWORD})
        check(st in (200, 201), f"register: {st} {reg}")
        jwt = reg["token"]
        st, pat = http(base, "POST", "/api/v1/users/tokens", jwt, body={"name": "mcp-client", "scopes": "repo"})
        check(st in (200, 201), f"create PAT: {st} {pat}")
        pat_token = pat["token"]
        say("one account, two credentials", "client A: repo-scoped PAT; client B: login JWT")

        st, _ = http(base, "POST", "/api/v1/repos", jwt, body={"name": REPO, "default_branch": "main"})
        check(st == 201, "create repo")
        seed = os.path.join(work, "seed")
        os.makedirs(seed)
        git(["init", "-q", "--initial-branch=main"], seed)
        with open(os.path.join(seed, "parser.rs"), "w") as f:
            f.write("fn parse() {}\n")
        git(["add", "."], seed)
        git(["commit", "-q", "-m", "initial parser"], seed)
        real_sha = git(["rev-parse", "HEAD"], seed)
        git(["push", "-q", f"{base}/git/{OWNER}/{REPO}", "main"], seed, token=jwt)
        say("real git repository populated through the server", f"commit {real_sha[:12]}")
        st, issue = http(base, "POST", f"/api/v1/repos/{OWNER}/{REPO}/issues", jwt, body={"title": "parser crashes on empty input"})
        check(st == 201, "create issue")

        collab = f"/api/v1/repos/{OWNER}/{REPO}/collaboration"

        # Client B registers by REST and keeps its own secret.
        st, b_reg = http(base, "POST", collab + "/sessions", jwt, body={"display_name": "Harness B", "client_kind": "http-script"})
        check(st == 201, f"B register: {st} {b_reg}")
        b = {"id": b_reg["session"]["id"], "secret": b_reg["session_secret"]}
        secrets_seen.append(b["secret"])
        say("B: registered session over REST", b["id"])

        # Client A starts through MCP; the secret never reaches its output.
        session_file = os.path.join(work, "mcp-sessions.json")
        mcp = McpClient(base, pat_token, session_file, work, "1")
        a_start = mcp.ok("collab_start_session", display_name="Harness A", client_kind="mcp")
        check("session_secret" not in json.dumps(a_start), "MCP output must not carry the secret")
        a_id = a_start["id"]
        with open(session_file) as f:
            a_secret = json.load(f)[f"{OWNER}/{REPO}"]["secret"]
        secrets_seen.append(a_secret)
        check(a_id != b["id"], "distinct sessions")
        say("A: started session through MCP; secret held by the client, not the model", a_id)

        sessions = mcp.ok("collab_list_sessions")
        ids = {s["id"]: s for s in sessions["items"]}
        check(a_id in ids and b["id"] in ids, "A can discover B and itself")
        check(all(s["origin"] == "user_content" for s in ids.values()), "labels marked untrusted")
        say("A: discovered B", f"{ids[b['id']]['display_name']} ({ids[b['id']]['state']})")

        # A claims a task, linking the real issue and declaring paths/branch.
        task = mcp.ok("collab_create_task", title="Fix parser crash", description="see issue",
                      owner_session_id="self", branch="fix/parser", paths=["parser.rs"], issue_number=issue["number"])
        task_id = task["id"]
        check(task["owner_session_id"] == a_id and task["version"] == 1, "A owns the task")
        say("A: created and claimed a task", f"{task_id} v{task['version']}")

        # A posts progress with a real commit and a blocker addressed to B.
        m1 = mcp.ok("collab_send_message", task_id=task_id, kind="note", body="Reproduced the crash; fix in progress.", commit_sha=real_sha)
        check(m1["seq"] == 1 and m1["commit_sha"] == real_sha, "first message")
        is_error, bogus = mcp.tool("collab_send_message", task_id=task_id, kind="note", body="x", commit_sha="a" * 40)
        check(is_error and bogus["error"]["status"] == 400, "a declared commit that does not exist is refused")
        say("A: a commit claim is checked for existence only", "bogus SHA rejected; real SHA stored as a claim")
        blk = mcp.ok("collab_send_message", task_id=task_id, kind="blocker", body="CI fails on the tokenizer; need a second pair of eyes.",
                     target_session_id=b["id"], client_message_id="a-blocker-1")
        check(blk["seq"] == 2, "blocker seq")
        retry = mcp.ok("collab_send_message", task_id=task_id, kind="blocker", body="CI fails on the tokenizer; need a second pair of eyes.",
                       target_session_id=b["id"], client_message_id="a-blocker-1")
        check(retry["id"] == blk["id"], "retry with the same client id stores one message")
        say("A: posted a blocker for B; a retry did not duplicate it", f"seq {blk['seq']}")

        # B finds the task without any copy-pasted chat.
        st, tasks = http(base, "GET", collab + "/tasks?status=open", jwt)
        check(st == 200 and any(t["id"] == task_id for t in tasks["items"]), "B discovers the task by itself")
        st, msgs = http(base, "GET", f"{collab}/tasks/{task_id}/messages?after_seq=0", jwt)
        check(st == 200 and [m["seq"] for m in msgs["items"]] == [1, 2], "B reads the stream")
        b_cursor = msgs["next_after_seq"]
        targeted = [m for m in msgs["items"] if m["target_session_id"] == b["id"]]
        check(len(targeted) == 1 and targeted[0]["kind"] == "blocker", "B sees the blocker addressed to it")
        st, wm = http(base, "PUT", f"{collab}/tasks/{task_id}/read", jwt, b, {"last_read_seq": b_cursor})
        check(st == 200 and wm["last_read_seq"] == 2, "B marks read")
        st, reply = http(base, "POST", f"{collab}/tasks/{task_id}/messages", jwt, b, {
            "client_message_id": "b-reply-1", "kind": "note", "reply_to_id": blk["id"],
            "body": "Looking at the tokenizer now; taking over the investigation."})
        check(st == 201 and reply["seq"] == 3, f"B replies: {st} {reply}")
        st, t = http(base, "PATCH", f"{collab}/tasks/{task_id}", jwt, b, {"expected_version": task["version"], "status": "blocked"})
        check(st == 200 and t["version"] == 2 and t["status"] == "blocked", "B declares the task blocked")
        st, stale = http(base, "PATCH", f"{collab}/tasks/{task_id}", jwt, b, {"expected_version": 1, "status": "in_progress"})
        check(st == 409 and stale["error"]["code"] == "VERSION_CONFLICT" and stale["error"]["details"]["current_version"] == 2,
              "a stale update is refused with the current version")
        say("B: discovered, read, replied, changed status; stale write refused", f"task v{t['version']}")

        # A crashes. The MCP process is killed without ending its session.
        mcp.crash()
        say("A: MCP process killed (crash)")

        # Meanwhile B keeps working; then the whole service restarts.
        st, m4 = http(base, "POST", f"{collab}/tasks/{task_id}/messages", jwt, b, {
            "client_message_id": "b-progress-1", "kind": "note", "body": "Found it: off-by-one in the tokenizer."})
        check(st == 201 and m4["seq"] == 4, "B posts while A is away")
        server.stop()
        server.start()
        say("server restarted", "same database")

        # A comes back: same session resumes, nothing needs to be copied over.
        mcp = McpClient(server.base, pat_token, session_file, work, "2")
        resumed = mcp.ok("collab_start_session", display_name="Harness A", client_kind="mcp")
        check(resumed["resumed"] is True and resumed["id"] == a_id, "A resumes the same session after crash and restart")
        sessions_now = mcp.ok("collab_list_sessions")["items"]
        check(len([s for s in sessions_now if s["display_name"] == "Harness A"]) == 1, "no duplicate session")
        missed = mcp.ok("collab_list_messages", task_id=task_id, after_seq=2)
        check([m["seq"] for m in missed["items"]] == [3, 4], "A reads exactly what it missed")
        check(missed["next_after_seq"] == 4, "cursor advances")
        mark = mcp.ok("collab_mark_read", task_id=task_id, last_read_seq=4)
        check(mark["last_read_seq"] == 4, "A's read position")
        say("A: resumed after crash+restart, read the missed messages", "seq 3..4, cursor 4")

        # Hand the task over to B; only B can accept.
        ho = mcp.ok("collab_create_handoff", task_id=task_id, to_session_id=b["id"], summary="Crash reproduced; tokenizer off-by-one suspected.",
                    completed="Reproduction and issue triage.", next_steps="Fix tokenizer, add regression test.",
                    blockers="CI red until the fix lands.", commit_sha=real_sha, evidence=[f"issue:{issue['number']}", f"commit:{real_sha}"])
        check(ho["status"] == "pending" and ho["version"] == 1 and ho["origin"] == "user_content", "handoff pending")
        is_error, wrong = mcp.tool("collab_accept_handoff", handoff_id=ho["id"], expected_version=1)
        check(is_error and wrong["error"]["status"] == 403, "the creator cannot accept its own handoff")
        st, pending = http(base, "GET", f"{collab}/tasks/{task_id}/handoffs", jwt)
        check(st == 200 and pending["items"][0]["status"] == "pending" and pending["items"][0]["commit_sha"] == real_sha, "B sees the pending handoff")
        st, accepted = http(base, "POST", f"{collab}/handoffs/{ho['id']}/accept", jwt, b, {"expected_version": 1})
        check(st == 200 and accepted["status"] == "accepted" and accepted["version"] == 2, f"B accepts: {st} {accepted}")
        is_error, late = mcp.tool("collab_cancel_handoff", handoff_id=ho["id"], expected_version=2)
        check(is_error and late["error"]["status"] == 409, "a finished handoff cannot be cancelled")
        st, t_after = http(base, "GET", f"{collab}/tasks/{task_id}", jwt)
        check(t_after["owner_session_id"] == a_id and t_after["version"] == 2, "accepting a handoff does not reassign the task or alter its declared state")
        say("A handed over, B accepted; the handoff alone changed nothing else", f"handoff {ho['id']} accepted")

        # B takes ownership explicitly (its own declared action) and finishes.
        st, t3 = http(base, "PATCH", f"{collab}/tasks/{task_id}", jwt, b, {"expected_version": 2, "owner_session_id": b["id"], "status": "done"})
        check(st == 200 and t3["owner_session_id"] == b["id"] and t3["status"] == "done", "B claims and completes")
        say("B: claimed the task and declared it done", "a declaration, not a verification")

        # A's final catch-up shows the whole history, ordered, with origin marks.
        full = mcp.ok("collab_list_messages", task_id=task_id, after_seq=0, limit=100)
        check([m["seq"] for m in full["items"]] == [1, 2, 3, 4], "ordered, gap-free history")
        check(all(m["origin"] == "user_content" for m in full["items"]), "messages marked as untrusted content")
        check(full["items"][0]["commit_sha"] == real_sha, "SHA is kept as a claim next to the message, not as proof")
        mcp.ok("collab_end_session")
        st, hb = http(base, "POST", f"{collab}/sessions/{b['id']}/heartbeat", jwt, b)
        check(st == 200, "B still alive")
        st, _ = http(base, "POST", f"{collab}/sessions/{b['id']}/end", jwt, b)
        check(st == 204, "B ends")
        mcp.close()
        mcp = None
        say("both sessions ended", "history remains readable")

        # No secret anywhere it should not be.
        corpus = {"server log": server.log_text()}
        for name in os.listdir(work):
            if name.startswith("mcp-") and name.endswith(".stderr"):
                with open(os.path.join(work, name), "rb") as f:
                    corpus[name] = f.read().decode("utf-8", "replace")
        for where, text in corpus.items():
            for secret in secrets_seen:
                check(secret not in text, f"a session secret leaked into {where}")
            check(not re.search(r"ifs_[0-9a-f]{64}", text), f"secret-shaped value in {where}")
        sizes = ", ".join(f"{k}={len(v)}B" for k, v in corpus.items())
        check(len(corpus["server log"]) > 2000, "server log is too small for the leak scan to mean anything")
        say("no session secret in the server log or MCP process logs", f"{len(corpus)} logs scanned ({sizes})")

        summary = {
            "task_id": task_id, "commit_sha": real_sha, "sessions": {"A": a_id, "B": b["id"]},
            "final_task": t3, "messages": [m["seq"] for m in full["items"]],
        }
        say("T12 scenario complete")
        return summary
    finally:
        if mcp is not None:
            try:
                mcp.crash()
            except Exception:
                pass
        server.stop()


def scan_for_secrets(server, work, peer=None):
    """No session secret may appear in the server log or any MCP process log."""
    corpus = {"server log": server.log_text()}
    if peer is not None:
        corpus["peer server log"] = peer.log_text()
    for name in os.listdir(work):
        if name.startswith("mcp-") and name.endswith(".stderr"):
            with open(os.path.join(work, name), "rb") as f:
                corpus[name] = f.read().decode("utf-8", "replace")
    for where, text in corpus.items():
        for secret in secrets_seen:
            check(secret not in text, f"a session secret leaked into {where}")
        check(not re.search(r"ifs_[0-9a-f]{64}", text), f"secret-shaped value in {where}")
    check(len(corpus["server log"]) > 2000, "server log is too small for the leak scan to mean anything")
    return ", ".join(f"{k}={len(v)}B" for k, v in corpus.items())


def run_p0(work, keep):
    """P15: A (MCP) works and asks, crashes; B (REST) is put in charge by a user,
    the user answers, B writes a work package; a fresh MCP client continues from
    the records alone. Real server, real Git repository, no mocks."""
    server = Server(work)
    mcp = None
    mcp_c = None
    try:
        server.start()
        base = server.base
        say("server started", f"{base} (sqlite, {platform.system()} {platform.machine()})")
        st, reg = http(base, "POST", "/api/v1/users/register",
                       body={"username": OWNER, "email": f"{OWNER}@example.com", "password": PASSWORD})
        check(st in (200, 201), f"register: {st} {reg}")
        jwt = reg["token"]  # the user door: no session headers
        st, pat = http(base, "POST", "/api/v1/users/tokens", jwt, body={"name": "mcp-client", "scopes": "repo"})
        check(st in (200, 201), f"create PAT: {st} {pat}")
        pat_token = pat["token"]
        st, _ = http(base, "POST", "/api/v1/repos", jwt, body={"name": REPO, "default_branch": "main"})
        check(st == 201, "create repo")

        seed = os.path.join(work, "seed")
        os.makedirs(os.path.join(seed, "web"))
        git(["init", "-q", "--initial-branch=main"], seed)
        for rel, text in [("parser.rs", "fn parse() {}\n"), ("AGENTS.md", "# Rules\nRun cargo test.\n"),
                          ("web/AGENTS.md", "# Web rules\nUse pnpm.\n")]:
            with open(os.path.join(seed, rel), "w") as f:
                f.write(text)
        git(["add", "."], seed)
        git(["commit", "-q", "-m", "initial"], seed)
        sha1 = git(["rev-parse", "HEAD"], seed)
        git(["push", "-q", f"{base}/git/{OWNER}/{REPO}", "main", "main:fix/parser"], seed, token=jwt)
        say("real git repository populated through the server", f"commit {sha1[:12]} on main and fix/parser")

        collab = f"/api/v1/repos/{OWNER}/{REPO}/collaboration"
        session_file = os.path.join(work, "mcp-sessions.json")
        mcp = McpClient(base, pat_token, session_file, work, "a")
        a_start = mcp.ok("collab_start_session", display_name="Harness A", client_kind="mcp")
        a_id = a_start["id"]
        with open(session_file) as f:
            secrets_seen.append(json.load(f)[f"{OWNER}/{REPO}"]["secret"])
        st, b_reg = http(base, "POST", collab + "/sessions", jwt, body={"display_name": "Harness B", "client_kind": "http-script"})
        check(st == 201, f"B register: {st} {b_reg}")
        b = {"id": b_reg["session"]["id"], "secret": b_reg["session_secret"]}
        secrets_seen.append(b["secret"])
        say("A (MCP) and B (REST) registered", f"A={a_id[:8]} B={b['id'][:8]}")

        # A finds the rules for the area it will touch, pinned to a commit.
        rules = mcp.ok("collab_get_rules", path="web/src")
        check(rules["resolved_commit"] == sha1, "rules are pinned to the commit")
        check([e["path"] for e in rules["entries"]] == ["AGENTS.md", "web/AGENTS.md"], f"rules for web/src: {rules}")
        check(all(e["origin"] == "repository_content" for e in rules["entries"]), "rules are repository content")
        say("A: located the rule files for web/src", "AGENTS.md, web/AGENTS.md at " + sha1[:12])

        task = mcp.ok("collab_create_task", title="Fix parser crash", owner_session_id="self", branch="fix/parser", paths=["parser.rs"])
        task_id = task["id"]
        q = mcp.ok("collab_ask_question", task_id=task_id, recipient={"kind": "repo_writers"},
                   title="Keep the cache in SQLite?", body="The parser cache can stay in SQLite or move to PostgreSQL. Which?",
                   options=["keep sqlite", "switch to postgresql"], context={"head_sha": sha1})
        check(q["status"] == "open" and q["creator_kind"] == "session" and q["options"][1]["id"] == "o2", "A asked")
        say("A: created a task and asked the user a question", f"question {q['id'][:8]} at task v{q['context']['task_version']}")

        mcp.crash()
        say("A: MCP process killed (crash); its session is still online")

        # B may not just take the task: A could still be working.
        st, claim = http(base, "POST", f"{collab}/tasks/{task_id}/takeovers", jwt, b, {
            "client_takeover_id": "b-claim-1", "expected_task_version": 1, "expected_owner_session_id": a_id, "reason": "A crashed"})
        check(st == 409 and claim["error"]["code"] == "OWNER_MAY_BE_ACTIVE", f"claim refused while A may be active: {st} {claim}")
        check(claim["error"]["details"]["owner_state"] == "online", "the refusal names why")
        say("B: claim refused while the owner may still be active", claim["error"]["code"])

        # A user reassigns, with a reason; the record names the door, not a person.
        st, re = http(base, "POST", f"{collab}/tasks/{task_id}/reassign", jwt, None, {
            "client_takeover_id": "user-reassign-1", "expected_task_version": 1, "expected_owner_session_id": a_id,
            "to_session_id": b["id"], "reason": "A's harness died; B continues"})
        check(st == 201 and re["task"]["owner_session_id"] == b["id"] and re["takeover"]["actor_kind"] == "user", f"reassign: {st} {re}")
        st, again = http(base, "POST", f"{collab}/tasks/{task_id}/reassign", jwt, None, {
            "client_takeover_id": "user-reassign-1", "expected_task_version": 1, "expected_owner_session_id": a_id,
            "to_session_id": b["id"], "reason": "A's harness died; B continues"})
        check(st == 200 and again["replayed"] is True, "a retried reassignment is a replay")
        say("a user reassigned the task to B; a retry was a no-op", f"task v{re['task']['version']}")

        # B's inbox and the user's inbox: work found without copying any chat.
        st, b_inbox = http(base, "GET", collab + "/inbox", jwt, b)
        types = sorted(i["type"] for i in b_inbox["items"])
        check(st == 200 and types == ["assigned_task", "open_question"], f"B inbox: {b_inbox}")
        check(b_inbox["consistency"] == "live_query", "the inbox says it is live")
        st, u_inbox = http(base, "GET", collab + "/inbox/user", jwt)
        check(st == 200 and [i["type"] for i in u_inbox["items"]] == ["open_question"], f"user inbox: {u_inbox}")
        say("B and the user each found what waits for them", "B: task + question; user: question")

        # The user reads the question with its context and answers.
        st, qq = http(base, "GET", f"{collab}/questions/{q['id']}", jwt)
        check(st == 200 and qq["context_freshness"] in ("fresh", "stale"), f"question: {qq}")
        # The reassignment moved the task to v2: the question's context is outdated,
        # so the platform refuses an answer given against the old context.
        check(qq["context_freshness"] == "stale", "the task moved on after the question was asked")
        st, refused = http(base, "POST", f"{collab}/questions/{q['id']}/answer", jwt, None, {
            "client_answer_id": "user-ans-1", "expected_version": qq["version"], "option_id": "o1",
            "acknowledged_context_hash": qq["context_hash"]})
        check(st == 409 and refused["error"]["code"] == "CONTEXT_STALE", f"stale context blocks the answer: {st} {refused}")
        say("the user's answer was refused: the context changed since the question", refused["error"]["code"])

        # A new question against the current context is answerable.
        st, q2 = http(base, "POST", f"{collab}/tasks/{task_id}/questions", jwt, b, {
            "client_question_id": "b-q-1", "recipient": {"kind": "repo_writers"},
            "title": "Keep the cache in SQLite?", "body": "Still SQLite or PostgreSQL?",
            "options": ["keep sqlite", "switch to postgresql"], "context": {"head_sha": sha1}})
        check(st == 201, f"B re-asks: {st} {q2}")
        st, q2r = http(base, "GET", f"{collab}/questions/{q2['id']}", jwt)
        check(q2r["context_freshness"] == "fresh", "the new question is fresh")
        st, ans = http(base, "POST", f"{collab}/questions/{q2['id']}/answer", jwt, None, {
            "client_answer_id": "user-ans-2", "expected_version": q2r["version"], "option_id": "o1",
            "acknowledged_context_hash": q2r["context_hash"]})
        check(st == 200 and ans["status"] == "answered" and ans["answer"]["actor_kind"] == "user", f"user answers: {st} {ans}")
        st, other = http(base, "POST", f"{collab}/questions/{q2['id']}/answer", jwt, None, {
            "client_answer_id": "user-ans-3", "expected_version": q2r["version"], "option_id": "o2",
            "acknowledged_context_hash": q2r["context_hash"]})
        check(st == 409 and other["error"]["code"] == "QUESTION_NOT_OPEN", "a second answer finds the question closed")
        say("the user answered B's question with the context in view", "option o1; a second answer was refused")

        # B pushes real work on the task's branch and writes a work package.
        with open(os.path.join(seed, "parser.rs"), "w") as f:
            f.write("fn parse() { /* empty input handled */ }\n")
        git(["commit", "-q", "-am", "handle empty input"], seed)
        sha2 = git(["rev-parse", "HEAD"], seed)
        git(["push", "-q", f"{base}/git/{OWNER}/{REPO}", "main:fix/parser"], seed, token=jwt)
        st, wp = http(base, "POST", f"{collab}/tasks/{task_id}/work-packages", jwt, b, {
            "client_package_id": "b-wp-1", "goal": "Fix the parser crash on empty input", "scope": "parser.rs",
            "commit_sha": sha2, "completed": ["empty input handled"], "open_items": ["regression test"],
            "next_steps": ["add a regression test", "run CI"],
            "evidence_refs": [{"kind": "commit", "reference": sha2}, {"kind": "note", "reference": "tests pass locally, trust me"}]})
        check(st == 201 and wp["revision"] == 1 and wp["commit_sha"] == sha2, f"work package: {st} {wp}")
        say("B pushed a commit and wrote a work package", f"revision 1 at {sha2[:12]}")

        st, t = http(base, "GET", f"{collab}/tasks/{task_id}", jwt)
        st, _ = http(base, "PATCH", f"{collab}/tasks/{task_id}", jwt, b, {"expected_version": t["version"], "status": "done"})
        check(st == 200, "B declares done (never blocked)")
        st, comp = http(base, "GET", f"{collab}/tasks/{task_id}/completion-status", jwt)
        by = {c["id"]: c for c in comp["conditions"]}
        check(comp["declared_status"] == "done" and comp["declared_done_without_evidence"] is True, f"done without evidence is flagged: {comp}")
        check(by["ci_passed_for_commit"]["status"] == "unknown", "no CI ran for this commit: unknown, not passed")
        check(by["work_package_present"]["status"] == "satisfied" and by["questions_resolved"]["status"] == "unsatisfied",
              "package present; the unanswered first question still counts as open")
        check(comp["observed_branch_head"] == sha2, "the platform observed the branch head itself")
        say("completion status separates the 'done' claim from the evidence", "CI unknown, 1 question still open")

        # A fresh MCP client continues from the records alone.
        mcp_c = McpClient(base, pat_token, os.path.join(work, "mcp-sessions-c.json"), work, "c")
        c_start = mcp_c.ok("collab_start_session", display_name="Harness C", client_kind="mcp")
        with open(os.path.join(work, "mcp-sessions-c.json")) as f:
            secrets_seen.append(json.load(f)[f"{OWNER}/{REPO}"]["secret"])
        check(c_start["id"] not in (a_id, b["id"]), "C is a new session; it inherits no secret")
        pkgs = mcp_c.ok("collab_list_work_packages", task_id=task_id)
        check(len(pkgs["items"]) == 1 and pkgs["items"][0]["commit_sha"] == sha2, "C finds B's package")
        pkg = mcp_c.ok("collab_get_work_package", package_id=pkgs["items"][0]["id"])
        check(pkg["evidence_resolution"] == [], "no pipeline references: nothing is claimed as verified")
        check(pkg["missing_fields"] == [], f"complete package: {pkg['missing_fields']}")
        got = mcp_c.ok("collab_get_question", question_id=q2["id"])
        check(got["status"] == "answered" and got["answer"]["option_id"] == "o1", "C reads the user's decision")
        takeovers = mcp_c.ok("collab_list_takeovers", task_id=task_id)
        check([x["outcome"] for x in takeovers["items"]] == ["reassigned"], "C sees who moved the task and why")
        comp_c = mcp_c.ok("collab_get_completion_status", task_id=task_id)
        check(comp_c["declared_done_without_evidence"] is True, "C sees the same evidence gap")
        say("C (fresh MCP client) continued from the records alone", "package, decision, history and evidence gap all readable")

        # What a deletion would take, for the record.
        st, impact = http(base, "GET", f"{collab}/deletion-impact", jwt)
        check(st == 200 and impact["on_repository_delete"] == "kept" and impact["records"]["questions"] == 2, f"impact: {impact}")
        say("deletion preview", f"{impact['records']['tasks']} task, {impact['records']['questions']} questions kept on soft delete")

        mcp_c.close()
        mcp_c = None
        sizes = scan_for_secrets(server, work)
        say("no session secret in the server log or MCP process logs", sizes)
        say("P15 scenario complete")
        return {"task_id": task_id, "commits": [sha1, sha2], "sessions": {"A": a_id, "B": b["id"], "C": c_start["id"]},
                "questions": [q["id"], q2["id"]], "work_package": wp["id"]}
    finally:
        for client in (mcp, mcp_c):
            if client is not None:
                try:
                    client.crash()
                except Exception:
                    pass
        server.stop()


def run_x04(work, keep, db_url=None, multi_instance=False):
    """X04/M12: real MCP and REST clients, two private Git repositories (X02)."""
    server = Server(work, db_url)
    peer = None
    mcp = None
    remote_owner, remote_repo = "x04-owner", "private-target"
    try:
        server.start()
        base = server.base
        if multi_instance:
            peer = Server(work, server.db_url, "server-peer")
            peer.start()
        base2 = peer.base if peer else base
        def register(name):
            st, result = http(base, "POST", "/api/v1/users/register", body={
                "username": name, "email": f"{name}@example.com", "password": PASSWORD})
            check(st == 201, f"register {name}: {st}")
            return result
        account, owner = register(OWNER), register(remote_owner)
        jwt, owner_jwt = account["token"], owner["token"]
        st, pat = http(base, "POST", "/api/v1/users/tokens", jwt,
                       body={"name": "x04-mcp", "scopes": "repo"})
        check(st == 201, "create MCP token")
        st, anchor = http(base, "POST", "/api/v1/repos", jwt,
                          body={"name": REPO, "is_private": True})
        check(st == 201, "create anchor")
        st, target = http(base, "POST", "/api/v1/repos", owner_jwt,
                          body={"name": remote_repo, "is_private": True})
        check(st == 201, "create target")
        target_path = f"/api/v1/repos/{remote_owner}/{remote_repo}"
        st, _ = http(base, "POST", target_path + "/collaborators", owner_jwt,
                     body={"user_id": account["user_id"], "permission": "write"})
        check(st == 201, "grant target write access")
        commits = {}
        for label, repo_owner, repo_name, token in (("anchor", OWNER, REPO, jwt),
                                                    ("target", remote_owner, remote_repo, owner_jwt)):
            seed = os.path.join(work, "seed-" + label)
            os.makedirs(seed)
            git(["init", "-q", "--initial-branch=main"], seed)
            with open(os.path.join(seed, "fixture.txt"), "w") as f:
                f.write(label + " real git fixture\n")
            git(["add", "."], seed)
            git(["commit", "-q", "-m", label], seed)
            commits[label] = git(["rev-parse", "HEAD"], seed)
            git(["push", "-q", f"{base}/git/{repo_owner}/{repo_name}", "main"], seed, token=token)
        say("two private Git repositories created", "shared account owns anchor and has target write access")

        collab = f"/api/v1/repos/{OWNER}/{REPO}/collaboration"
        target_collab = target_path + "/collaboration"
        st, caps = http(base, "GET", "/api/v1/collaboration/capabilities", jwt)
        check(st == 200 and caps["contract_version"].split(".")[0] == "1", "compatible instance capabilities")
        group_caps = next((c for c in caps["capabilities"] if c["name"] == "groups"), None)
        check(group_caps is not None, "groups capability advertised")
        for path in (collab, target_collab):
            st, rc = http(base, "GET", path + "/capabilities", jwt)
            check(st == 200 and rc["caller"] == {"repo_collab_read": True, "repo_write": True},
                  "repository capabilities reflect write access")
        session_file = os.path.join(work, "x04-mcp-sessions.json")
        mcp = McpClient(base, pat["token"], session_file, work, "x04")
        mcp_info = mcp.server_info
        tools = mcp.rpc("tools/list", {})["result"]["tools"]
        tool_map = {t["name"]: t for t in tools}
        check(all(name in tool_map for name in group_caps["mcp_tools"]), "advertised group MCP tools exist")
        check("target_repo_id" in tool_map["collab_add_group_link"]["inputSchema"]["properties"],
              "group link schema accepts target repository id")
        say("capabilities discovered", f"contract {caps['contract_version']}, server {caps['server_version']}, MCP group tools present")

        a = mcp.ok("collab_start_session", display_name="X04 MCP", client_kind="mcp")
        with open(session_file) as f:
            a_secret = json.load(f)[f"{OWNER}/{REPO}"]["secret"]
        secrets_seen.append(a_secret)
        st, reg_b = http(base, "POST", collab + "/sessions", jwt,
                         body={"display_name": "X04 REST", "client_kind": "http-script"})
        check(st == 201, "register REST anchor session")
        b = {"id": reg_b["session"]["id"], "secret": reg_b["session_secret"]}
        secrets_seen.append(b["secret"])
        st, reg_target = http(base, "POST", target_collab + "/sessions", jwt,
                              body={"display_name": "X04 REST target", "client_kind": "http-script"})
        check(st == 201, "register independent target session")
        bt = {"id": reg_target["session"]["id"], "secret": reg_target["session_secret"]}
        secrets_seen.append(bt["secret"])
        check(len({a["id"], b["id"], bt["id"]}) == 3 and len({a_secret, b["secret"], bt["secret"]}) == 3,
              "sessions and secrets are distinct")
        st, wrong = http(base, "POST", target_collab + f"/sessions/{b['id']}/heartbeat", jwt, b)
        check(st == 403, "anchor session cannot act in target")
        st, task = http(base, "POST", target_collab + "/tasks", jwt, bt,
                        {"title": "target-private-task"})
        check(st == 201, "create target task")
        group = mcp.ok("collab_create_group", title="X04 coordination")
        link = mcp.ok("collab_add_group_link", group_id=group["id"], target_repo_id=target["id"],
                      target_task_id=task["id"], commit_sha=commits["target"], expected_group_version=group["version"])
        st, page = http(base2, "GET", collab + f"/groups/{group['id']}/links", jwt)
        check(st == 200 and len(page["items"]) == 1 and page["items"][0]["id"] == link["id"],
              "REST sees MCP-created target link")
        say("MCP linked the target task and real commit; REST discovered it")

        cursor = None
        if peer:
            st, task2 = http(base2, "POST", target_collab + "/tasks", jwt, bt,
                             {"title": "second target task"})
            check(st == 201, "create another target task through the peer")
            link2 = mcp.ok("collab_add_group_link", group_id=group["id"], target_repo_id=target["id"],
                           target_task_id=task2["id"], expected_group_version=group["version"] + 1)
            st, first = http(base, "GET", collab + f"/groups/{group['id']}/links?limit=1", jwt)
            check(st == 200 and first["has_more"] is True and first["next_cursor"], "first page supplies a cursor")
            cursor = first["next_cursor"]
            st, second = http(base2, "GET", collab + f"/groups/{group['id']}/links?limit=1&cursor={cursor}", jwt)
            check(st == 200 and len(second["items"]) == 1 and second["has_more"] is False
                  and {first["items"][0]["id"], second["items"][0]["id"]} == {link["id"], link2["id"]},
                  "cursor minted on one instance continues on the other")
            say("group cursor continued across server processes", "both visible links returned once")

        anchor_task = mcp.ok("collab_create_task", title="X04 continuation", owner_session_id="self")
        sent = mcp.ok("collab_send_message", task_id=anchor_task["id"], kind="note",
                      body="Continue from this record", target_session_id=b["id"])
        st, _ = http(base2, "POST", collab + f"/tasks/{anchor_task['id']}/messages/{sent['id']}/read-receipt", jwt, b)
        check(st == 201, "REST recipient confirms MCP author's targeted message")
        delivery = mcp.ok("collab_get_delivery", task_id=anchor_task["id"], message_id=sent["id"])
        check(delivery["status"] == "read", "MCP author sees REST recipient's receipt")
        st, addressed = http(base2, "POST", collab + f"/tasks/{anchor_task['id']}/messages", jwt, b,
                             {"kind": "note", "body": "Message to MCP", "client_message_id": "x04-targeted",
                              "target_session_id": a["id"]})
        check(st == 201, "REST sends a message to MCP session")
        notices = mcp.ok("collab_list_notices")
        notice = next((n for n in notices["items"] if n["source_id"] == addressed["id"]), None)
        check(notice is not None, "MCP sees its addressed notice")
        acked = mcp.ok("collab_ack_notice", notice_id=notice["id"])
        check(acked["newly_acked"] is True, "MCP acknowledges its own notice")
        receipt = mcp.ok("collab_confirm_receipt", task_id=anchor_task["id"], message_id=addressed["id"])
        check(receipt["newly_confirmed"] is True, "MCP confirms REST author's message")
        st, delivery = http(base2, "GET", collab + f"/tasks/{anchor_task['id']}/messages/{addressed['id']}/delivery", jwt, b)
        check(st == 200 and delivery["status"] == "read", "REST author sees MCP receipt")
        say("MCP and REST exchanged notices and receipts", "acknowledgement and read receipt remain separate")
        mcp.ok("collab_mark_read", task_id=anchor_task["id"], last_read_seq=1)
        mcp.crash()
        mcp = None
        st, unread = http(base2, "PUT", collab + f"/tasks/{anchor_task['id']}/read", jwt, b, {"last_read_seq": 0})
        check(st == 200 and unread["last_read_seq"] == 0, "REST session did not inherit MCP read watermark")
        st, messages = http(base2, "GET", collab + f"/tasks/{anchor_task['id']}/messages?after_seq=0", jwt)
        check(st == 200 and [x["seq"] for x in messages["items"]] == [1, 2], "REST reads disconnected client's records")
        st, reply = http(base2, "POST", collab + f"/tasks/{anchor_task['id']}/messages", jwt, b,
                         {"kind": "note", "body": "REST continues from the shared record", "client_message_id": "x04-reply"})
        check(st == 201 and reply["seq"] == 3, "REST continues after MCP disconnect")
        say("REST continued after MCP disconnect", "independent secret and read watermark")

        st, changed = http(base2, "PATCH", collab + f"/tasks/{anchor_task['id']}", jwt, b,
                           {"expected_version": anchor_task["version"], "status": "blocked"})
        check(st == 200 and changed["status"] == "blocked", "REST records a blocked task")
        st, health = http(base2, "GET", collab + "/health", jwt)
        metrics = {item["id"]: item for item in health.get("metrics", [])} if st == 200 else {}
        check(st == 200 and "blocked_tasks" in metrics and "computed_at" in health
              and metrics["blocked_tasks"].get("value", {}).get("blocked_at_to") == 1,
              f"anchor administrator reads health metrics with coverage: {st} {health}")
        st, _ = http(base, "GET", collab + "/health", owner_jwt)
        check(st in (403, 404), "an account without anchor administration cannot read health metrics")
        say("REST checked collaboration health", "blocked task event; admin-only access")

        if peer:
            # Both processes share durable state, not local locks. Race writes
            # from separate HTTP connections through independent server processes.
            path = collab + f"/tasks/{anchor_task['id']}"
            barrier = threading.Barrier(2, timeout=10)
            def change(base_url, state):
                barrier.wait()
                return http(base_url, "PATCH", path, jwt, b,
                            {"expected_version": changed["version"], "status": state})
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                futures = [pool.submit(change, base, "waiting"), pool.submit(change, base2, "in_progress")]
                race = [future.result() for future in futures]
            check(sorted(st for st, _ in race) == [200, 409], f"cross-instance task CAS: {race}")
            message_path = path + "/messages"
            payload = {"kind": "note", "body": "cross-instance replay", "client_message_id": "x04-cross-instance-id"}
            barrier = threading.Barrier(2, timeout=10)
            def send_replay(base_url):
                barrier.wait()
                return http(base_url, "POST", message_path, jwt, b, payload)
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                futures = [pool.submit(send_replay, url) for url in (base, base2)]
                retries = [future.result() for future in futures]
            check(sorted(st for st, _ in retries) == [200, 201]
                  and retries[0][1]["id"] == retries[1][1]["id"], f"cross-instance replay: {retries}")
            say("independent servers arbitrated concurrent requests", "one CAS winner; one idempotent message")

        st, _ = http(base, "DELETE", target_path + f"/collaborators/{account['user_id']}", owner_jwt)
        check(st == 204, "revoke target access")
        st, page = http(base2, "GET", collab + f"/groups/{group['id']}/links", jwt)
        check(st == 200 and page["items"] == [] and page["has_more"] is False and page["next_cursor"] is None,
              "REST filters target immediately after revocation")
        raw = json.dumps(page)
        check(all(s not in raw for s in (remote_repo, task["id"], "target-private-task", commits["target"], link["id"])),
              "filtered response leaks no target data")
        check(not any(k in page for k in ("total", "count", "hidden")), "filtered response exposes no hidden count")
        if cursor:
            st, after_revoke = http(base2, "GET", collab + f"/groups/{group['id']}/links?limit=1&cursor={cursor}", jwt)
            check(st == 200 and after_revoke["items"] == [] and after_revoke["next_cursor"] is None,
                  "a cross-instance cursor cannot retain access after revocation")
        st, _ = http(base2, "GET", target_collab + "/capabilities", jwt)
        check(st == 404, "private target capabilities denied after revocation")
        st, _ = http(base2, "POST", target_collab + f"/sessions/{bt['id']}/heartbeat", jwt, bt)
        check(st in (403, 404), "existing target session cannot bypass revocation")
        mcp = McpClient(base, pat["token"], session_file, work, "x04-restart")
        filtered = mcp.ok("collab_list_group_links", group_id=group["id"])
        check(filtered["items"] == [] and filtered["has_more"] is False, "MCP also filters revoked target")
        mcp.close()
        mcp = None
        say("target permission revoked", "REST and MCP filter link; target capabilities and existing session denied")
        if peer:
            server.stop()
            st, restored = http(base2, "GET", collab + f"/tasks/{anchor_task['id']}", jwt)
            check(st == 200 and restored["id"] == anchor_task["id"], "peer serves durable task while primary is down")
            st, _ = http(base2, "POST", collab + f"/sessions/{b['id']}/heartbeat", jwt, b)
            check(st == 200, "peer accepts original session credentials after primary stops")
            server.start()
            st, restored = http(base, "GET", collab + f"/tasks/{anchor_task['id']}", jwt)
            check(st == 200 and restored["id"] == anchor_task["id"], "restarted primary reads the same task")
            say("server failover and restart", "peer retained task and session; restarted primary recovered state")
        say("session secret log scan passed", scan_for_secrets(server, work, peer))
        def binary_hash(path):
            with open(path, "rb") as f:
                return hashlib.file_digest(f, "sha256").hexdigest()
        backend = "postgresql" if server.db_url.startswith(("postgres:", "postgresql:")) else "mysql" if server.db_url.startswith("mysql:") else "sqlite"
        say("X04 scenario complete", f"{backend}, {'two instances' if peer else 'single instance'}")
        return {"scenario": "x04", "backend": backend, "instances": 2 if peer else 1, "scope": "X01 discovery, X02 groups and X03 health", "commits": commits,
                "repository_ids": {"anchor": anchor["id"], "target": target["id"]},
                "group_id": group["id"], "contract_version": caps["contract_version"],
                "server_version": caps["server_version"], "mcp_server_info": mcp_info,
                "binary_sha256": {"server": binary_hash(SERVER_BIN), "mcp": binary_hash(MCP_BIN)}}
    finally:
        if mcp is not None:
            mcp.crash()
        if peer is not None:
            peer.stop()
        server.stop()


def run_ci_compat(work, keep, db_url=None):
    """Real CI API, protected environment and approval; no job is executed."""
    server = Server(work, db_url, extra_args=["--external-runners"])
    try:
        server.start()
        base = server.base
        def register(name):
            st, result = http(base, "POST", "/api/v1/users/register", body={
                "username": name, "email": f"{name}@example.com", "password": PASSWORD})
            check(st == 201, f"register CI user: {st} {result}")
            return result
        owner, reviewer = register(OWNER), register("ci-reviewer")
        jwt, reviewer_jwt = owner["token"], reviewer["token"]
        st, _ = http(base, "POST", "/api/v1/repos", jwt,
                     body={"name": REPO, "is_private": True})
        check(st == 201, "create private CI test repository")
        repo_path = f"/api/v1/repos/{OWNER}/{REPO}"
        st, _ = http(base, "POST", repo_path + "/collaborators", jwt,
                     body={"user_id": reviewer["user_id"], "permission": "read"})
        check(st == 201, "grant CI reviewer read access")
        st, environment = http(base, "POST", repo_path + "/actions/environments", jwt,
                               body={"name": "ci-compat", "protected": True, "required_approvals": 2,
                                     "allowed_approver_ids": [owner["user_id"], reviewer["user_id"]]})
        check(st == 201, f"create protected environment: {st} {environment}")
        seed = os.path.join(work, "ci-seed")
        os.makedirs(seed)
        git(["init", "-q", "--initial-branch=main"], seed)
        with open(os.path.join(seed, ".ironforge-ci.yml"), "w") as f:
            f.write("stages: [verify]\ncompat_job:\n  stage: verify\n  script:\n    - echo never-executed\n  environment: ci-compat\n")
        git(["add", "."], seed)
        git(["commit", "-q", "-m", "CI compatibility fixture"], seed)
        sha = git(["rev-parse", "HEAD"], seed)
        git(["push", "-q", f"{base}/git/{OWNER}/{REPO}", "main"], seed, token=jwt)
        st, created = http(base, "POST", repo_path + "/pipelines", jwt, body={"ref_name": "refs/heads/main"})
        check(st == 201, f"trigger CI pipeline: {st} {created}")
        pipeline_id = created["id"]
        st, detail = http(base, "GET", repo_path + f"/pipelines/{pipeline_id}", jwt)
        check(st == 200 and len(detail["stages"]) == 1 and len(detail["stages"][0]["jobs"]) == 1,
              f"read stage and job: {st} {detail}")
        job = detail["stages"][0]["jobs"][0]
        check(detail["pipeline"]["status"] == "waiting_approval" and job["status"] == "waiting_approval"
              and job["environment_id"] == environment["id"], f"protected CI gate: {detail}")
        approve_path = repo_path + f"/pipelines/{pipeline_id}/jobs/{job['id']}/approve"
        st, first = http(base, "POST", approve_path, jwt)
        check(st == 200 and first["approvals"] == 1 and first["released"] is False, f"first approval: {st} {first}")
        st, _ = http(base, "POST", approve_path, jwt)
        check(st == 409, "duplicate approval refused")
        st, second = http(base, "POST", approve_path, reviewer_jwt)
        check(st == 200 and second["approvals"] == 2 and second["released"] is True,
              f"second approval releases job: {st} {second}")
        st, final = http(base, "GET", repo_path + f"/pipelines/{pipeline_id}", jwt)
        check(st == 200 and final["stages"][0]["jobs"][0]["status"] == "pending",
              f"released job remains pending without runners: {st} {final}")
        barrier = threading.Barrier(2, timeout=10)
        def trigger_again():
            barrier.wait()
            return http(base, "POST", repo_path + "/pipelines", jwt, body={"ref_name": "refs/heads/main"})
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            pending = [pool.submit(trigger_again) for _ in range(2)]
            outcomes = [future.result() for future in pending]
        check(all(st == 201 for st, _ in outcomes)
              and len({body["id"] for _, body in outcomes}) == 2,
              f"concurrent pipeline creation: {outcomes}")
        backend = "postgresql" if server.db_url.startswith(("postgres:", "postgresql:")) else "mysql" if server.db_url.startswith("mysql:") else "sqlite"
        say("real CI stage, job and approvals verified", f"{backend}; external-runner mode, no job executed")
        with open(SERVER_BIN, "rb") as f:
            server_hash = hashlib.file_digest(f, "sha256").hexdigest()
        return {"scenario": "ci-compat", "backend": backend, "commit": sha, "pipeline_id": pipeline_id,
                "stage_id": detail["stages"][0]["stage"]["id"], "job_id": job["id"],
                "scope": "external-runner mode; no job executed", "server_binary_sha256": server_hash}
    finally:
        server.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", choices=["t12", "p0", "x04", "ci-compat"], default="t12",
                        help="t12: first-version flow; p0: P15; x04: two-client P2; ci-compat: protected CI approval")
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    parser.add_argument("--multi-instance", action="store_true", help="X04: run two independent servers sharing the test DB and repository directory")
    parser.add_argument("--json-out", help="write a JSON summary with environment info")
    args = parser.parse_args()
    for binary in (SERVER_BIN, MCP_BIN):
        check(os.access(binary, os.X_OK), f"missing binary {binary}; run: cargo build -p rg-cli -p rg-mcp")

    work = tempfile.mkdtemp(prefix="ironforge-collab-e2e.")
    status = 1
    try:
        if args.multi_instance and args.scenario != "x04":
            check(False, "--multi-instance requires --scenario x04")
        db_url = os.environ.get("IRONFORGE_E2E_DB_URL")
        if db_url and args.scenario not in ("x04", "ci-compat"):
            check(False, "IRONFORGE_E2E_DB_URL requires --scenario x04 or ci-compat and a disposable database")
        if args.scenario == "x04":
            summary = run_x04(work, args.keep, db_url, args.multi_instance)
        elif args.scenario == "ci-compat":
            summary = run_ci_compat(work, args.keep, db_url)
        else:
            summary = {"t12": run, "p0": run_p0}[args.scenario](work, args.keep)
        status = 0
        if args.json_out:
            def sh(*cmd):
                return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True).stdout.strip()
            summary["environment"] = {
                "os": platform.platform(), "python": platform.python_version(),
                "git": sh("git", "--version"),
                "repo_head": sh("git", "rev-parse", "HEAD"),
                "working_tree_dirty": bool(sh("git", "status", "--porcelain")),
                "server_binary": SERVER_BIN, "database": summary.get("backend", "sqlite"),
            }
            summary["transcript"] = transcript
            with open(args.json_out, "w") as f:
                json.dump(summary, f, indent=2)
    finally:
        if args.keep or status != 0:
            print(f"work directory kept: {work}", file=sys.stderr)
        else:
            shutil.rmtree(work, ignore_errors=True)
    sys.exit(status)


if __name__ == "__main__":
    main()
