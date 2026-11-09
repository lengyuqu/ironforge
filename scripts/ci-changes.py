#!/usr/bin/env python3
"""Decide how much of the regression workflow a change needs.

Used by the `changes` job of .github/workflows/regression.yml. Standard library
only. Prints `key=value` lines (also appended to $GITHUB_OUTPUT when set):

  full           true  -> run every job and `cargo test --workspace`
  rust           true  -> clippy and the Rust test job run
  frontend       true  -> the frontend job runs
  db_smoke       true  -> the PostgreSQL / MySQL smoke jobs run
  git_protocol   true  -> the real Git client protocol matrix runs
  packages       "-p a -p b" for `cargo test` when full is false, else empty
  reason         one line saying why

Rules (see classify()):
  * push / workflow_dispatch / schedule, or anything that goes wrong while
    working the answer out        -> full (a warning is printed in the second
    case): the script fails closed.
  * pull requests: the files changed against the merge base are classified.
    Docs only -> nothing. web/ only -> frontend only. Changes confined to
    crates that rg-core / rg-db / rg-git do not feed (rg-ci, rg-ssh, rg-runner,
    rg-cli, rg-ci-isolation, rg-http, rg-mcp) -> those crates plus everything
    that depends on them, found from the Cargo manifests (no hard-coded
    graph). rg-core / rg-db / rg-git, migrations, Cargo.toml / Cargo.lock,
    .github/, scripts/, deploy/ and any file not recognised -> full.

Usage:
  ci-changes.py --event pull_request --base origin/main      # real run
  ci-changes.py --files a b c                                  # classify a list
  ci-changes.py --self-test                                    # local checks
"""

import argparse
import os
import subprocess
import sys

try:
    import tomllib
except ImportError:  # Python < 3.11: fall back to `cargo metadata`
    tomllib = None

# Crates whose change can alter the behaviour of nearly everything: no scoping.
CORE_CRATES = {"rg-core", "rg-db", "rg-git"}
# Top-level paths whose change always means a full run.
FULL_PREFIXES = (".github/", "scripts/", "deploy/")
FULL_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml"}
FULL_DIRS = (".cargo/",)
# Paths with no effect on any job.
DOC_PREFIXES = ("ironforge-docs/", "docs/", ".ai/")
DOC_SUFFIXES = (".md",)


def sh(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def repo_root():
    return sh("git", "rev-parse", "--show-toplevel").strip()


def members_from_toml(root):
    """{package name: (dir, set of path-dependency dirs)} from the manifests."""
    with open(os.path.join(root, "Cargo.toml"), "rb") as fh:
        workspace = tomllib.load(fh)["workspace"]["members"]
    manifests = {}
    for member in workspace:
        with open(os.path.join(root, member, "Cargo.toml"), "rb") as fh:
            manifests[member] = tomllib.load(fh)

    def dep_tables(doc):
        for key in ("dependencies", "dev-dependencies", "build-dependencies"):
            yield doc.get(key, {})
        for target in doc.get("target", {}).values():
            for key in ("dependencies", "dev-dependencies", "build-dependencies"):
                yield target.get(key, {})

    result = {}
    for member, doc in manifests.items():
        deps = set()
        for table in dep_tables(doc):
            for spec in table.values():
                if isinstance(spec, dict) and "path" in spec:
                    deps.add(os.path.normpath(os.path.join(member, spec["path"])))
        result[doc["package"]["name"]] = (member, deps)
    return result


def members_from_cargo(root):
    """The same shape, from `cargo metadata --no-deps` (no network needed)."""
    import json

    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            check=True, capture_output=True, text=True, cwd=root,
        ).stdout
    )
    result = {}
    for pkg in meta["packages"]:
        directory = os.path.relpath(os.path.dirname(pkg["manifest_path"]), root)
        deps = {
            os.path.relpath(dep["path"], root)
            for dep in pkg["dependencies"]
            if dep.get("path")
        }
        result[pkg["name"]] = (directory, deps)
    return result


def workspace_graph(root, prefer_cargo=False):
    loaders = [members_from_toml, members_from_cargo]
    if tomllib is None or prefer_cargo:
        loaders.reverse()
    last = None
    for loader in loaders:
        if loader is members_from_toml and tomllib is None:
            continue
        try:
            return loader(root)
        except Exception as exc:  # noqa: BLE001 - try the other source
            last = exc
    raise RuntimeError(f"cannot read the workspace: {last}")


def reverse_closure(graph, seeds):
    """`seeds` plus every workspace crate that depends on one of them."""
    dir_to_name = {directory: name for name, (directory, _) in graph.items()}
    dependents = {name: set() for name in graph}
    for name, (_, dep_dirs) in graph.items():
        for dep_dir in dep_dirs:
            dep = dir_to_name.get(dep_dir)
            if dep:
                dependents[dep].add(name)
    seen, todo = set(seeds), list(seeds)
    while todo:
        for parent in dependents.get(todo.pop(), ()):
            if parent not in seen:
                seen.add(parent)
                todo.append(parent)
    return seen


def full(reason):
    return {
        "full": True, "rust": True, "frontend": True, "db_smoke": True,
        "git_protocol": True, "packages": [], "reason": reason,
    }


def classify(files, graph):
    """Map the changed files to what must run. Anything unclear -> full."""
    if not files:
        return full("no changed files found (treated as unknown)")
    dir_to_name = {directory: name for name, (directory, _) in graph.items()}
    crates, web, unknown = set(), False, []
    for path in files:
        path = path.replace("\\", "/")
        if path.startswith(FULL_PREFIXES) or path.startswith(FULL_DIRS) or path in FULL_FILES:
            return full(f"{path} affects every job")
        crate = next(
            (name for directory, name in dir_to_name.items() if path.startswith(directory + "/")),
            None,
        )
        if crate:
            if crate in CORE_CRATES:
                return full(f"{path} is in {crate}, which most crates depend on")
            crates.add(crate)
        elif path.startswith("web/"):
            web = True
        elif path.startswith(DOC_PREFIXES) or ("/" not in path and path.endswith(DOC_SUFFIXES)):
            continue
        else:
            unknown.append(path)
    if unknown:
        return full(f"unclassified path {unknown[0]} (and {len(unknown) - 1} more)" if len(unknown) > 1
                    else f"unclassified path {unknown[0]}")
    affected = reverse_closure(graph, crates) if crates else set()
    if any(name in CORE_CRATES for name in affected):  # cannot happen, kept as a guard
        return full("a core crate is affected")
    parts = []
    if affected:
        parts.append("crates " + ", ".join(sorted(crates)))
    if web:
        parts.append("web/")
    if not parts:
        parts.append("docs only")
    return {
        "full": False,
        "rust": bool(affected),
        "frontend": web,
        # The database smoke tests live in rg-core, which is a core crate, so a
        # scoped run never needs them; the Git protocol matrix runs the whole
        # binary, so it runs whenever rg-cli is in the affected set.
        "db_smoke": False,
        "git_protocol": "rg-cli" in affected,
        "packages": sorted(affected),
        "reason": "; ".join(parts)
        + (f" -> test {', '.join(sorted(affected))}" if affected else ""),
    }


def changed_files(base, head="HEAD"):
    merge_base = sh("git", "merge-base", base, head).strip()
    out = sh("git", "diff", "--name-only", "--no-renames", merge_base, head)
    return [line for line in out.splitlines() if line]


def decide(event, base, root, files=None, head="HEAD"):
    if files is None and event != "pull_request":
        return full(f"{event or 'unknown event'} always runs everything")
    try:
        graph = workspace_graph(root)
        if files is None:
            if not base:
                raise RuntimeError("no base ref given")
            files = changed_files(base, head)
        return classify(files, graph)
    except Exception as exc:  # noqa: BLE001 - fail closed
        print(f"::warning::could not scope this run ({exc}); running everything")
        return full(f"fail-closed: {exc}")


def render(result):
    flag = lambda value: "true" if value else "false"  # noqa: E731
    return [
        f"full={flag(result['full'])}",
        f"rust={flag(result['rust'])}",
        f"frontend={flag(result['frontend'])}",
        f"db_smoke={flag(result['db_smoke'])}",
        f"git_protocol={flag(result['git_protocol'])}",
        "packages=" + " ".join(f"-p {name}" for name in result["packages"]),
        "reason=" + result["reason"].replace("\n", " "),
    ]


def self_test(root):
    """Checks that need no network and no particular git history."""
    graph = workspace_graph(root)
    if tomllib is not None:
        try:
            via_cargo = workspace_graph(root, prefer_cargo=True)
        except Exception:  # noqa: BLE001
            via_cargo = None
        if via_cargo is not None:
            assert via_cargo == graph, "Cargo.toml parsing and `cargo metadata` disagree"
    cases = [
        (["README.md", "ironforge-docs/x.md", ".ai/y.md"], dict(full=False, rust=False, frontend=False, packages=[])),
        (["web/src/a.svelte"], dict(full=False, rust=False, frontend=True, packages=[])),
        (["web/src/a.svelte", "CLAUDE.md"], dict(full=False, rust=False, frontend=True, packages=[])),
        (["crates/rg-http/src/lib.rs"], dict(full=False, rust=True, packages=["rg-cli", "rg-http"])),
        (["crates/rg-mcp/src/lib.rs"], dict(full=False, rust=True, packages=["rg-cli", "rg-http", "rg-mcp"])),
        (["crates/rg-ssh/src/lib.rs"], dict(full=False, rust=True, packages=["rg-cli", "rg-ssh"])),
        (["crates/rg-runner/src/main.rs"], dict(full=False, rust=True, packages=["rg-runner"])),
        (["crates/rg-ci-isolation/src/lib.rs"], dict(full=False, rust=True,
                                                    packages=["rg-ci", "rg-ci-isolation", "rg-cli", "rg-runner"])),
        (["crates/rg-http/README.md"], dict(full=False, rust=True)),
        (["crates/rg-core/src/lib.rs"], dict(full=True)),
        (["crates/rg-db/src/migration/x.rs"], dict(full=True)),
        (["crates/rg-git/src/lib.rs"], dict(full=True)),
        (["Cargo.lock"], dict(full=True)),
        (["crates/rg-http/Cargo.toml", "web/a.ts"], dict(full=False, rust=True, frontend=True)),
        (["scripts/ci-db-tests.sh"], dict(full=True)),
        (["deploy/docker-compose.yml"], dict(full=True)),
        ([".github/workflows/regression.yml"], dict(full=True)),
        (["something-new/at-the-root.txt"], dict(full=True)),
        (["docs-only.md", "unknown.bin"], dict(full=True)),
        ([], dict(full=True)),
    ]
    for files, expect in cases:
        got = classify(files, graph)
        for key, value in expect.items():
            assert got[key] == value, f"{files}: {key} = {got[key]!r}, expected {value!r}"
    # rg-cli is in a scoped set only through its dependencies, and so is the protocol matrix
    assert classify(["crates/rg-http/src/lib.rs"], graph)["git_protocol"] is True
    assert classify(["crates/rg-runner/src/main.rs"], graph)["git_protocol"] is False
    assert decide("push", None, root)["full"] is True
    assert decide("workflow_dispatch", None, root)["full"] is True
    assert decide("schedule", None, root)["full"] is True
    assert decide("pull_request", "refs/does/not/exist", root)["full"] is True  # fail closed
    print("self-test ok")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--event", default=os.environ.get("EVENT_NAME", ""))
    parser.add_argument("--base", default="", help="base revision of a pull request, e.g. origin/main")
    parser.add_argument("--head", default="HEAD", help="the changed side of the range (default HEAD)")
    parser.add_argument("--files", nargs="*", help="classify this list instead of running git")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    try:
        root = repo_root()
    except Exception as exc:  # noqa: BLE001
        print(f"::warning::not a git checkout ({exc}); running everything")
        result = full("fail-closed: no git checkout")
        root = None
    if args.self_test:
        self_test(root)
        return
    if root is not None:
        result = decide(args.event if args.files is None else "pull_request",
                        args.base, root, args.files, args.head)
    lines = render(result)
    print("\n".join(lines))
    target = os.environ.get("GITHUB_OUTPUT")
    if target:
        with open(target, "a", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
