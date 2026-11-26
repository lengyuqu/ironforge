<p align="center"><img src="assets/brand/ironforge-logo.svg" alt="IronForge" width="288"></p>

# IronForge 🔨

> **Git hosting for AI agents** — built from scratch in Rust, shipped as a single binary.

**English** | [简体中文](README.zh-CN.md)

[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

IronForge is a Git hosting platform built for agents. It ships the hosting baseline — repositories, issues, pull requests, wiki, CI/CD, a package registry, SSO/LDAP, audit logs and code search — in one Rust binary. It is not another Gitea clone; it answers a different question:

> **When the main writers are concurrent, retrying, token-billed agents, what should a code hosting platform look like?**

Every design choice is tested against one question: does it let an agent use fewer tokens, act more predictably, and leave a trace a human can audit?

## Why agents need a different platform

1. **Tokens cost money.** How much one read returns, and how many round trips it takes, decides both cost and success rate.
2. **Agents are concurrent and retry.** Several agents edit the same repository; network jitter or a model retry sends the same request twice.
3. **They work through machine-readable feedback.** Why CI failed, why a merge conflicts, why a permission was denied must be structured fields, not an HTML page or a log a model has to guess at.
4. **They should not be fully trusted.** Agents can be prompt-injected by issue bodies, PR text or logs. The platform has to limit what they *can* do instead of assuming good behaviour.

## What IronForge provides

| Area | What it does |
|---|---|
| **Agent as a first-class principal** | An agent is its own account (`kind = agent`) owned by a person, with permissions capped by the owner. It has no human login path, cannot approve PRs, create repositories or organizations; approval counts include only people; audit records the owner on the main write paths, while commit author strings are content, not proof of who acted. |
| **Least-privilege tokens** | PAT scope ladder `repo:read` < `repo:collab` < `repo:write` < `repo`. Tokens can be restricted to repositories (`repos`) and branch patterns (`refs`). Branch-limited tokens cannot merge, approve PRs or use new write routes by default. |
| **Previewable, concurrent, retryable writes** | File commits, PR creation and merges support `dry_run`, `expected_sha` (409 `REF_ADVANCED` with the current commit when the branch moved) and `Idempotency-Key`. Batch commits turn N file changes into **one commit = one CI run**, all-or-nothing; cherry-pick and revert; errors carry `reason` / `retryable` / `hint`. |
| **Short CI loop** | Long-polling pipeline waits, incremental/tail logs, a `failure_summary` on failed jobs; a persisted repository event log with long polling (no public webhook URL needed); `get_pr_context` with a token budget; merge readiness (conflict files, every branch-protection gate) and `update-branch`; full PR export for replay and evaluation. |
| **Token-efficient code reading** | AI endpoints (repository summary, paged file tree, code search), a symbol-definition index with `find_definition`, and file reads with a 2,000-line / 128 KiB window. |
| **Multi-agent collaboration** | Sessions in the same repository coordinate around **tasks**: messages, blockers, handoffs, structured questions and answers, immutable decisions, dependencies, path-overlap warnings, context packs for reconnects, completion conditions, cross-repository groups and health metrics. Humans join through the web UI and notifications. |
| **Native MCP** | `ironforge-mcp` exposes **77 tools** (18 repository/PR/CI tools + 59 `collab_*` tools, as reported by `tools/list`) over stdio and streamable HTTP. The remote entry point holds no credentials; token scopes and allowlists still apply. **There is no merge tool** — the agent flow is "commit to `agent/*` → open a PR → wait for CI → a human merges". |
| **Trust boundary in the contract** | Client-reported fields (session name, task state, handoff summary, comment bodies, …) are marked `origin: user_content`: data, not instructions. A claim is not evidence, and "done" is not completion. The capability endpoint `/api/v1/collaboration/capabilities` advertises only what the build actually has; write requests reject unknown fields. |

### A typical agent loop

```text
1. get_pr_context / find_definition / read_file      read context (budgeted, truncation is explicit)
2. commit_files  -> agent/fix-login                  N files = one commit = one CI run (expected_sha + idempotency key)
3. create_pr                                        open a PR (dry_run first if you like)
4. wait_pipeline                                    long-poll CI; on failure read failure_summary and go back to 2
5. collab_ask_question / collab_create_handoff       record a decision or hand off instead of guessing
6. a human reviews and merges                       the agent cannot merge or approve
```

## Connect an agent

```bash
# stdio (local agents such as Claude Code / Cursor / Cline)
IRONFORGE_URL=http://localhost:8080 IRONFORGE_PAT=<token> ironforge-mcp

# streamable HTTP (remote agents; each connection carries its own token)
IRONFORGE_URL=http://127.0.0.1:8080 ironforge-mcp --http 127.0.0.1:8765
```

Create a dedicated agent account and token with `POST /api/v1/agents` and `POST /api/v1/agents/{username}/tokens` (humans only). Issue the smallest scope that works and add `repos` / `refs` restrictions. The full MCP setup, tool list, collaboration protocol and token rules are in [.ai/README.md](.ai/README.md); agent guardrails for this repository are in [.ai/guardrails.md](.ai/guardrails.md).

## `ifg` — the terminal client

`ifg` is the command-line client for IronForge: repositories, issues, PRs, CI, releases, code context and multi-agent collaboration from a terminal, with stable machine-readable output for scripts and agents. It is a separate MIT-licensed project: [lengyuqu/ifg](https://github.com/lengyuqu/ifg).

```sh
brew install lengyuqu/ifg/ifg        # macOS / Linux via Homebrew
ifg auth login --host http://localhost:8080
ifg doctor                            # host, token, scopes, server version, streaming
```

Common commands:

```sh
ifg repo list
ifg pr list -R owner/name
ifg pr readiness 42
ifg pr checkout 42
ifg ci logs <job-id> --follow
ifg context pr 42
ifg collab task list -R owner/name
```

- `--json` prints the server's own field names, so the same `jq` expressions work; `-q/--template` renders a small template per result.
- `--no-input` (or `IFG_NO_INPUT=1`) makes any prompt fail immediately with exit code 2 and names the flag to pass instead. It is not `--yes`: destructive commands still need `--yes`.
- `ifg schema` describes the command tree as JSON for agents, without network or login; `ifg api <path>` reaches the long tail (wiki, webhooks, packages, secrets, …).
- Inside a checkout `ifg` infers `OWNER/NAME` from `origin`; outside, pass `-R`.
- Other platforms: see the [ifg repository](https://github.com/lengyuqu/ifg) for install options.

## Quick start

Requirements:

- Rust 1.95+ (stable)
- Node.js 22+ for the web build and regression scripts
- `git` only for mirror sync and test fixtures; the production Git path is native gix
- macOS or Linux

```bash
git clone <this-repo>
cd ironforge
cargo build --release
```

The binary is at `target/release/ironforge`.

Create an SSH host key:

```bash
ssh-keygen -t ed25519 -f /tmp/ironforge_host_key -N ""
```

Start the server:

```bash
mkdir -p /tmp/ironforge/repos
./target/release/ironforge serve \
  --repo-root /tmp/ironforge/repos \
  --http-addr 0.0.0.0:8080 \
  --ssh-addr  0.0.0.0:2222 \
  --host-key  /tmp/ironforge_host_key \
  --db-url    "sqlite:///tmp/ironforge/ironforge.db?mode=rwc" \
  --jwt-secret my-secret-key
```

HTTP defaults to `0.0.0.0:8080` and SSH to `0.0.0.0:2222`. Migrations run at startup. Run `ironforge serve --help` for TLS, logging, SMTP, Docker runner and rate-limit flags.

Create a repository for a quick test (no database row — for protocol checks):

```bash
./target/release/ironforge create-repo testuser testrepo \
  --repo-root /tmp/ironforge/repos
```

Set `RUST_LOG=debug` for verbose logs.

## Docker

The repository ships a multi-stage `Dockerfile` (SvelteKit frontend + Rust release + slim runtime) and a root `docker-compose.yml`.

```bash
export IRONFORGE_JWT_SECRET=$(openssl rand -hex 32)
docker compose up -d --build

docker compose exec ironforge ironforge create-user admin \
  --email you@example.com --password 'change-me-please' --admin \
  --db-url "sqlite:///data/ironforge.db?mode=rwc"

curl -fsS http://localhost:8080/health
```

- Ports: `8080` (HTTP + web UI), `2222` (git over SSH).
- Data: the `ironforge-data` volume (repos, SQLite, logs) survives container rebuilds.
- PostgreSQL: start the optional service with `docker compose --profile postgres up -d` and override the app command as documented in the compose file.
- The image has a `HEALTHCHECK`; `docker compose ps` shows `healthy`.

Once the container is up, log the CLI in:

```bash
ifg auth login --host http://localhost:8080
ifg doctor
```

## API at a glance

REST lives under `/api/v1` and accepts a bearer JWT or PAT. Git Smart HTTP lives under `/git/<owner>/<repo>/…`. OpenAPI: `/api-docs/openapi.json`; Swagger UI: `/api-docs/` (protected).

| Area | Endpoint |
|---|---|
| Users and sessions | `/api/v1/users/register`, `/login`, `/me` |
| Repositories | `/api/v1/repos`, `/repos/{owner}/{name}` |
| Issues | `/repos/{owner}/{name}/issues` (comments, labels, milestones) |
| Pull requests | `/repos/{owner}/{name}/pulls` (diff, merge, readiness, update-branch) |
| AI context | `/ai/repos/{owner}/{name}/summary`, `/tree`, `/map`, `/symbols`, `/search/code` |
| PR context | `/ai/repos/{owner}/{name}/pulls/{number}/context` |
| Collaboration | `/repos/{owner}/{name}/collaboration/…` |
| CI | pipelines, jobs, logs, artifacts and environments under `/repos/{owner}/{name}/…` |

For the long tail use the OpenAPI document or `ifg api <path>`.

## Git over SSH and HTTP

```bash
# SSH (port 2222)
GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null" \
  git clone ssh://git@localhost:2222/testuser/testrepo /tmp/myrepo

# HTTP
git clone http://localhost:8080/git/testuser/testrepo /tmp/myrepo-http
```

Both support protocol v1 and v2, shallow and partial clone, and push over receive-pack. The real-client matrix is in [docs/git-client-matrix.md](docs/git-client-matrix.md).

## Development

```bash
scripts/check.sh --quick   # docs + fmt + clippy
scripts/check.sh           # + rg-http / rg-core tests
scripts/check.sh --full    # whole workspace, same as CI
```

The documentation gate is `python3 scripts/check-docs.py`: version alignment, links, index coverage and stale-path references. It runs in `check.sh` and in the CI `Docs consistency` job.

Frontend:

```bash
cd web
pnpm install
pnpm dev
```

End-to-end regression: `node scripts/full-interface-regression.mjs` (backend, frontend and runtime smoke). See [CONTRIBUTING.md](CONTRIBUTING.md) for crate boundaries and test rules.

## Project layout

```text
ironforge/
├── crates/
│   ├── rg-cli/             # main binary (bin = "ironforge")
│   ├── rg-core/            # business logic (auth/repo/issue/pr/wiki/lfs/...)
│   ├── rg-git/             # Git protocol (pkt-line/V1/V2/gix)
│   ├── rg-ssh/             # SSH server (russh)
│   ├── rg-http/            # HTTP server + REST API (axum)
│   ├── rg-db/              # database layer (SeaORM, SQLite/PostgreSQL/MySQL)
│   ├── rg-ci/              # CI engine (YAML + pipeline executor)
│   ├── rg-ci-isolation/    # container hardening shared by both runners
│   ├── rg-runner/          # external CI runner agent
│   └── rg-mcp/             # MCP server (stdio + streamable HTTP)
├── web/                    # SvelteKit SPA
├── docs/                   # index + current design + protocol/performance/operations
└── .ai/                    # agent integration guide and guardrails
```

## Tech stack

| Layer | Choice |
|---|---|
| Async runtime | tokio 1 |
| HTTP | axum 0.8 |
| SSH | russh 0.65 |
| Git | gix 0.91 (native production path; mirror sync is the only git-CLI exception) |
| Database | SeaORM 1.1 over SQLite, PostgreSQL or MySQL |
| Auth | argon2 + JWT (HS256) + LDAP/SSO + TOTP |
| Frontend | SvelteKit 5 SPA with English and Chinese i18n |
| MCP | Rust, stdio and streamable HTTP |

## Status and limitations

Current release: **0.5.1**. See [CHANGELOG.md](CHANGELOG.md) for the version history.

- **Single instance only.** Multi-instance deployment and rolling upgrades are not supported: the database, repository storage, startup migrations and periodic jobs are not externalised. See [docs/operations.md](docs/operations.md).
- **Repository permissions are still three-tier** (read / write / admin). Unit-level permissions are designed but not implemented (see [docs/design/authorization.md](docs/design/authorization.md) §9).
- **Third-party actions:** only `composite` is expanded. `node*` and `docker` actions fail closed until an `actions-v1` runner exists.
- **Memory:** the target is <50 MB; measured idle is ~84 MiB and a large-clone peak is 299 MiB. See [docs/performance.md](docs/performance.md).
- Feature boundaries and known gaps: see the known-gaps section of [CLAUDE.md](CLAUDE.md).

## Documentation

| Document | Purpose |
|---|---|
| [AGENT.md](AGENT.md) | compact entry point for AI tools |
| [CLAUDE.md](CLAUDE.md) | conventions, current implementation, pitfalls, troubleshooting |
| [ARCHITECTURE.md](ARCHITECTURE.md) | architecture and technical choices |
| [CONTRIBUTING.md](CONTRIBUTING.md) | crate boundaries, coding and test rules |
| [.ai/README.md](.ai/README.md) | MCP / REST integration and tool list |
| [docs/README.md](docs/README.md) | documentation index and rules |
| [docs/releasing.md](docs/releasing.md) | release process and documentation gate |
| [docs/design/](docs/design/) | current design: collaboration, authorization, agents, CI, data, code intelligence |

## License

[MIT](LICENSE) © 2026 Yu Qu (lengyuqu). Commercial and closed-source use is allowed; keep the copyright and permission notice when redistributing.
