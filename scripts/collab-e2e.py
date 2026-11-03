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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    parser.add_argument("--json-out", help="write a JSON summary with environment info")
    args = parser.parse_args()
    for binary in (SERVER_BIN, MCP_BIN):
        check(os.access(binary, os.X_OK), f"missing binary {binary}; run: cargo build -p rg-cli -p rg-mcp")

    work = tempfile.mkdtemp(prefix="ironforge-collab-e2e.")
    status = 1
    try:
        summary = run(work, args.keep)
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
