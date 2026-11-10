#!/usr/bin/env python3
"""Decide how much of the regression workflow a change needs.

Used by the `changes` job of .github/workflows/regression.yml. Standard library
only. Prints `key=value` lines (also appended to $GITHUB_OUTPUT when set):

  full           true  -> run every job and `cargo test --workspace`
  rust           true  -> clippy and the Rust test job run
  frontend       true  -> the frontend job runs
  db_smoke       true  -> the PostgreSQL / MySQL smoke jobs run
  git_protocol   true  -> the real Git client protocol matrix runs
  deploy         true  -> the Docker compose check runs
  packages       cargo test arguments when full is false: "-p a -p b", or
                 "--workspace --exclude rg-http"; empty when full is true
  reason         one line saying why

Rules (see classify()):
  * A full run happens only on `workflow_dispatch` (the owner starts it by hand
    when a change is big enough to warrant it). Pushes and nightly schedules
    are no longer triggers; if one reaches this script anyway it is treated as
    a full run, and so is anything that goes wrong while working out the
    answer (a warning is printed): the script fails closed.
  * Pull requests: the files changed against the merge base are classified.
    Docs only -> nothing, and so is a `.cnb.yml`-only change (CNB's own
    workflow file; GitHub never reads it, and an agent branch edits it next to
    its docs). web/ only -> the frontend only. Changes in crates ->
    clippy for the workspace and the tests of those crates and of everything
    that depends on them, found from the Cargo manifests (no hard-coded
    graph). Cargo.toml / Cargo.lock, .github/, scripts/, deploy/ and any file
    not recognised -> every crate's tests.
  * rg-http holds the heaviest integration suite (about 13 of the 24 minutes of
    a full Rust job), so a pull request runs it only when a file of rg-http
    itself changed. When it is merely downstream of the change it is left out
    and a notice says so; start the workflow by hand to run it.

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

# The crate with the heavy integration suite: tested on a pull request only when
# its own files changed.
HEAVY_CRATE = "rg-http"
# Crates that hold the database code: a change there also runs the PostgreSQL and
# MySQL smoke jobs.
DB_CRATES = {"rg-core", "rg-db"}
# Top-level paths and files that can affect every crate.
WORKSPACE_PREFIXES = (".github/", "scripts/", "deploy/", ".cargo/")
WORKSPACE_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml"}
CARGO_FILES = {"Cargo.toml", "Cargo.lock"}
# Paths with no effect on any job.
DOC_PREFIXES = ("ironforge-docs/", "docs/", ".ai/")
DOC_SUFFIXES = (".md",)
# CI configuration that this workflow must ignore. `.cnb.yml` drives CNB only
# (the development upstream) and .github/ drives this workflow, so a `.cnb.yml`
# edit is not a code change: without this entry it is unclassified, and an
# unclassified path runs every crate.
CI_ONLY_FILES = {".cnb.yml"}


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
        "git_protocol": True, "deploy": True, "packages": [], "workspace_wide": False,
        "skipped_heavy": False, "reason": reason,
    }


def classify(files, graph):
    """Map the changed files to what a pull request runs. Anything unclear is
    treated as touching the whole workspace (never silently as nothing)."""
    if not files:
        return {**full("no changed files"), "full": False, "rust": False, "frontend": False,
                "db_smoke": False, "git_protocol": False, "deploy": False}
    dir_to_name = {directory: name for name, (directory, _) in graph.items()}
    crates, web, wide, deploy, cargo_files, unknown = set(), False, False, False, False, []
    for path in files:
        path = path.replace("\\", "/")
        crate = next(
            (name for directory, name in dir_to_name.items() if path.startswith(directory + "/")),
            None,
        )
        if path.startswith(WORKSPACE_PREFIXES) or path in WORKSPACE_FILES:
            wide = True
            deploy = deploy or path.startswith((".github/", "deploy/"))
            cargo_files = cargo_files or path in CARGO_FILES
        elif crate:
            crates.add(crate)
        elif path.startswith("web/"):
            web = True
        elif path.startswith(DOC_PREFIXES) or ("/" not in path and path.endswith(DOC_SUFFIXES)):
            continue
        elif path in CI_ONLY_FILES:
            continue
        else:
            unknown.append(path)
            wide = True
    affected = set(graph) if wide else reverse_closure(graph, crates)
    heavy_changed = HEAVY_CRATE in crates
    tested = sorted(affected - (set() if heavy_changed else {HEAVY_CRATE}))
    skipped_heavy = HEAVY_CRATE in affected and not heavy_changed
    if not affected:
        packages = []
    elif wide:
        packages = ["--workspace"] + ([] if heavy_changed else ["--exclude", HEAVY_CRATE])
    else:
        packages = [arg for name in tested for arg in ("-p", name)]
    parts = []
    if wide:
        what = f"unclassified path {unknown[0]}" if unknown else "workspace-wide files"
        parts.append(f"{what} -> every crate")
    elif crates:
        parts.append("crates " + ", ".join(sorted(crates)))
    if web:
        parts.append("web/")
    if not parts:
        parts.append("docs only")
    if affected:
        parts.append("test " + ", ".join(tested))
    if skipped_heavy:
        parts.append(f"{HEAVY_CRATE} integration tests NOT run (start the workflow by hand to run them)")
    return {
        "full": False,
        "rust": bool(affected),
        "frontend": web,
        "db_smoke": bool(crates & DB_CRATES) or cargo_files,
        "git_protocol": "rg-cli" in affected,
        "deploy": deploy,
        "packages": packages,
        "workspace_wide": wide,
        "skipped_heavy": skipped_heavy,
        "reason": "; ".join(parts),
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
        f"deploy={flag(result['deploy'])}",
        "packages=" + " ".join(result["packages"]),
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
        # CNB's own workflow file: GitHub never reads it, and a docs pull request
        # that also edits it stays a docs pull request
        ([".cnb.yml", "ironforge-docs/x.md"], dict(full=False, rust=False, frontend=False, deploy=False,
                                                    packages=[])),
        (["web/src/a.svelte"], dict(full=False, rust=False, frontend=True, packages=[])),
        (["web/src/a.svelte", "CLAUDE.md"], dict(full=False, rust=False, frontend=True, packages=[])),
        # rg-http itself changed: its integration suite runs
        (["crates/rg-http/src/lib.rs"], dict(full=False, rust=True, packages=["-p", "rg-cli", "-p", "rg-http"],
                                            skipped_heavy=False)),
        # rg-mcp feeds rg-http, but rg-http's own files did not change: left out
        (["crates/rg-mcp/src/lib.rs"], dict(full=False, rust=True, packages=["-p", "rg-cli", "-p", "rg-mcp"],
                                           skipped_heavy=True)),
        (["crates/rg-ssh/src/lib.rs"], dict(full=False, rust=True, packages=["-p", "rg-cli", "-p", "rg-ssh"],
                                           skipped_heavy=False)),
        (["crates/rg-runner/src/main.rs"], dict(full=False, rust=True, packages=["-p", "rg-runner"])),
        (["crates/rg-ci-isolation/src/lib.rs"], dict(full=False, rust=True, packages=[
            "-p", "rg-ci", "-p", "rg-ci-isolation", "-p", "rg-cli", "-p", "rg-runner"], skipped_heavy=False)),
        (["crates/rg-http/README.md"], dict(full=False, rust=True)),
        # core crates: everything downstream, minus rg-http, plus the database smoke jobs
        (["crates/rg-core/src/lib.rs"], dict(full=False, rust=True, db_smoke=True, git_protocol=True,
                                            skipped_heavy=True)),
        (["crates/rg-db/src/migration/x.rs"], dict(full=False, rust=True, db_smoke=True)),
        (["crates/rg-git/src/lib.rs"], dict(full=False, rust=True, db_smoke=False, git_protocol=True)),
        # workspace-wide files: every crate except rg-http
        (["Cargo.lock"], dict(full=False, rust=True, db_smoke=True, deploy=False,
                             packages=["--workspace", "--exclude", "rg-http"])),
        (["crates/rg-http/Cargo.toml", "web/a.ts"], dict(full=False, rust=True, frontend=True)),
        (["scripts/ci-db-tests.sh"], dict(full=False, rust=True, deploy=False,
                                         packages=["--workspace", "--exclude", "rg-http"])),
        (["deploy/docker-compose.yml"], dict(full=False, rust=True, deploy=True)),
        ([".github/workflows/regression.yml"], dict(full=False, rust=True, deploy=True,
                                                   packages=["--workspace", "--exclude", "rg-http"])),
        (["something-new/at-the-root.txt"], dict(full=False, rust=True,
                                                packages=["--workspace", "--exclude", "rg-http"])),
        (["docs-only.md", "unknown.bin"], dict(full=False, rust=True)),
        # rg-http changed AND something workspace-wide: the whole workspace, heavy suite included
        (["crates/rg-http/src/lib.rs", "Cargo.lock"], dict(full=False, rust=True, packages=["--workspace"],
                                                           skipped_heavy=False)),
        ([], dict(full=False, rust=False, frontend=False, packages=[])),
    ]
    for files, expect in cases:
        got = classify(files, graph)
        for key, value in expect.items():
            assert got[key] == value, f"{files}: {key} = {got[key]!r}, expected {value!r}"
    # rg-cli is in a scoped set only through its dependencies, and so is the protocol matrix
    assert classify(["crates/rg-http/src/lib.rs"], graph)["git_protocol"] is True
    assert full("x")["full"] is True and full("x")["packages"] == []
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
    if result.get("skipped_heavy"):
        print(f"::notice::{HEAVY_CRATE} integration tests are not part of this run; "
              "start the workflow by hand (workflow_dispatch) to run them")
    target = os.environ.get("GITHUB_OUTPUT")
    if target:
        with open(target, "a", encoding="utf-8") as fh:
            fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
