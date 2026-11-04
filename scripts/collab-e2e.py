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
  python3 scripts/collab-e2e.py [--keep] [--json-out FILE]

Environment: IRONFORGE_BIN (default target/debug/ironforge),
             IRONFORGE_MCP_BIN (default target/debug/ironforge-mcp).
"""

import argparse
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
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
    def __init__(self, work):
        self.work = work
        self.http_port = free_port()
        self.ssh_port = free_port()
        self.base = f"http://127.0.0.1:{self.http_port}"
        self.proc = None
        self.log = open(os.path.join(work, "server.log"), "ab")

    def start(self):
        os.makedirs(os.path.join(self.work, "repos"), exist_ok=True)
        self.proc = subprocess.Popen(
            [
                SERVER_BIN, "serve",
                "--repo-root", os.path.join(self.work, "repos"),
                "--http-addr", f"127.0.0.1:{self.http_port}",
                "--ssh-addr", f"127.0.0.1:{self.ssh_port}",
                "--host-key", os.path.join(self.work, "host-key"),
                "--db-url", f"sqlite://{self.work}/ironforge.db?mode=rwc",
                "--jwt-secret", JWT_SECRET,
            ],
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
        with open(os.path.join(self.work, "server.log"), "rb") as f:
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


def scan_for_secrets(server, work):
    """No session secret may appear in the server log or any MCP process log."""
    corpus = {"server log": server.log_text()}
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--scenario", choices=["t12", "p0"], default="t12",
                        help="t12: first-version flow; p0: takeover, questions, work packages, inbox (P15)")
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    parser.add_argument("--json-out", help="write a JSON summary with environment info")
    args = parser.parse_args()
    for binary in (SERVER_BIN, MCP_BIN):
        check(os.access(binary, os.X_OK), f"missing binary {binary}; run: cargo build -p rg-cli -p rg-mcp")

    work = tempfile.mkdtemp(prefix="ironforge-collab-e2e.")
    status = 1
    try:
        summary = (run_p0 if args.scenario == "p0" else run)(work, args.keep)
        status = 0
        if args.json_out:
            def sh(*cmd):
                return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True).stdout.strip()
            summary["environment"] = {
                "os": platform.platform(), "python": platform.python_version(),
                "git": sh("git", "--version"),
                "repo_head": sh("git", "rev-parse", "HEAD"),
                "working_tree_dirty": bool(sh("git", "status", "--porcelain")),
                "server_binary": SERVER_BIN, "database": "sqlite (throw-away file)",
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
