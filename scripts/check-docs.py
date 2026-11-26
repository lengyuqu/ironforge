#!/usr/bin/env python3
"""Release-time documentation consistency check (stdlib only).

Checks:
  1. every crate, web/package.json, CHANGELOG, CLAUDE.md, AGENT.md and the
     current design/operations/performance docs carry the same version;
  2. every markdown link that points inside the repository resolves;
  3. every file under docs/ is indexed by docs/README.md;
  4. docs/ file names contain no date and no "verification";
  5. no reference to a deleted historical path remains in text files.

Run locally before a release; CI runs the same script as the
"Docs consistency" job.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path

VERSION_RE = re.compile(r'^version\s*=\s*"(\d+\.\d+\.\d+)"', re.M)
CHANGELOG_RE = re.compile(r"^## \[(\d+\.\d+\.\d+)\]", re.M)
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
DATE_IN_NAME = re.compile(r"\d{4}-\d{2}")
STALE_RE = re.compile(
    r"ironforge-docs/"
    r"|docs/reports/"
    r"|docs/plans/(?:T\d|AUTH-403|CI-202|skills-registry)"
    r"|[A-Za-z0-9_-]+-verification-20\d\d"
    r"|gix-migration-assessment"
    r"|T33-authorization-map"
)
TEXT_EXT = {".md", ".rs", ".ts", ".py", ".mjs", ".sh", ".yml", ".yaml"}
SKIP_DIRS = {".git", "target", "node_modules", "dist", "build", ".workbuddy", ".claude", "templates"}

STATUS_DOCS = [
    "docs/operations.md",
    "docs/performance.md",
]


def iter_files(root, suffixes=None):
    for path in root.rglob("*"):
        if any(part in SKIP_DIRS for part in path.parts):
            continue
        if path.is_file() and (suffixes is None or path.suffix in suffixes):
            yield path


def read(path):
    return path.read_text(encoding="utf-8", errors="ignore")


def check_versions(root):
    errors = []
    crate_versions = {}
    for manifest in (root / "crates").glob("*/Cargo.toml"):
        match = VERSION_RE.search(read(manifest))
        if match:
            crate_versions[manifest.parent.name] = match.group(1)
    if not crate_versions:
        return ["no crate version found under crates/*/Cargo.toml"], None

    unique = sorted(set(crate_versions.values()))
    if len(unique) != 1:
        detail = ", ".join("%s=%s" % item for item in sorted(crate_versions.items()))
        return ["crate versions differ: " + detail], None
    version = unique[0]

    package_json = root / "web/package.json"
    if not package_json.exists():
        errors.append("web/package.json is missing")
    else:
        try:
            web_version = json.loads(read(package_json)).get("version")
        except json.JSONDecodeError as exc:
            errors.append("web/package.json is not valid JSON: %s" % exc)
        else:
            if web_version != version:
                errors.append("web/package.json is %s, expected %s" % (web_version, version))

    changelog = root / "CHANGELOG.md"
    if not changelog.exists():
        errors.append("CHANGELOG.md is missing")
    else:
        sections = CHANGELOG_RE.findall(read(changelog))
        if not sections:
            errors.append("CHANGELOG.md has no '## [x.y.z]' section")
        elif sections[0] != version:
            errors.append("CHANGELOG.md newest section is %s, expected %s" % (sections[0], version))

    for name in ("CLAUDE.md", "AGENT.md"):
        path = root / name
        if not path.exists():
            errors.append(name + " is missing")
        elif "\uff08" + version + "\uff09" not in read(path):
            errors.append(name + " does not carry the current version \uff08" + version + "\uff09")

    for rel in STATUS_DOCS:
        path = root / rel
        if not path.exists():
            errors.append(rel + " is missing")
        elif "\u72b6\u6001" not in read(path) or version not in read(path):
            errors.append(rel + " does not carry \u72b6\u6001\uff08" + version + "\uff09")

    design = root / "docs/design"
    if not design.exists():
        errors.append("docs/design/ is missing")
    else:
        for path in sorted(design.glob("*.md")):
            if path.name.startswith("adr-"):
                continue
            text = read(path)
            if "\u72b6\u6001" not in text or version not in text:
                errors.append(str(path.relative_to(root)) + " does not carry \u72b6\u6001\uff08" + version + "\uff09")

    return errors, version


def check_links(root):
    errors = []
    for path in iter_files(root, {".md"}):
        for match in LINK_RE.finditer(read(path)):
            target = match.group(1).split("#", 1)[0].strip()
            if not target or target.startswith(("http://", "https://", "mailto:", "#", "<")):
                continue
            if "{{" in target or "*" in target:
                continue
            resolved = (path.parent / target).resolve()
            if not resolved.exists():
                errors.append(str(path.relative_to(root)) + ": broken link " + target)
    return errors


def check_index(root):
    errors = []
    index = root / "docs/README.md"
    if not index.exists():
        return ["docs/README.md is missing"]
    text = read(index)
    for path in sorted((root / "docs").rglob("*.md")):
        if path == index:
            continue
        rel = path.relative_to(root / "docs").as_posix()
        if rel not in text:
            errors.append(str(path.relative_to(root)) + " is not indexed by docs/README.md")
    return errors


def check_names(root):
    errors = []
    for path in sorted((root / "docs").rglob("*.md")):
        if DATE_IN_NAME.search(path.name):
            errors.append(str(path.relative_to(root)) + " has a date in its name")
        if "verification" in path.name.lower():
            errors.append(str(path.relative_to(root)) + " is a verification record")
    return errors


def check_stale(root):
    errors = []
    skip = {root / "scripts/check-docs.py"}
    for path in iter_files(root, TEXT_EXT):
        if path in skip:
            continue
        for lineno, line in enumerate(read(path).splitlines(), 1):
            if STALE_RE.search(line):
                errors.append(
                    "%s:%d: references a deleted path: %s"
                    % (path.relative_to(root), lineno, line.strip()[:100])
                )
    return errors


def run_checks(root):
    errors = []
    version_errors, version = check_versions(root)
    errors += version_errors
    errors += check_links(root)
    errors += check_index(root)
    errors += check_names(root)
    errors += check_stale(root)
    return errors, version


def write_fixture(root, broken=False):
    (root / "crates/rg-cli").mkdir(parents=True)
    (root / "crates/rg-cli/Cargo.toml").write_text('[package]\nversion = "9.9.9"\n', encoding="utf-8")
    (root / "web").mkdir()
    (root / "web/package.json").write_text('{"version": "9.9.9"}', encoding="utf-8")
    (root / "CHANGELOG.md").write_text("# Changelog\n\n## [9.9.9] - 2026-01-01\n\n- test\n", encoding="utf-8")
    (root / "CLAUDE.md").write_text("# CLAUDE\n\n\u6700\u540e\u66f4\u65b0\uff1a2026-01-01\uff089.9.9\uff09\n", encoding="utf-8")
    (root / "AGENT.md").write_text("# AGENT\n\n\u6700\u540e\u66f4\u65b0\uff1a2026-01-01\uff089.9.9\uff09\n", encoding="utf-8")
    (root / "docs/design").mkdir(parents=True)
    (root / "docs/design/a.md").write_text("# A\n\n> \u72b6\u6001\uff089.9.9\uff09\n", encoding="utf-8")
    (root / "docs/operations.md").write_text("# Ops\n\n> \u72b6\u6001\uff089.9.9\uff09\n", encoding="utf-8")
    (root / "docs/performance.md").write_text("# Perf\n\n> \u72b6\u6001\uff089.9.9\uff09\n", encoding="utf-8")
    links = "| [a](design/a.md) | [ops](operations.md) | [perf](performance.md) |\n"
    if broken:
        links += "\n[bad](missing.md)\n"
    (root / "docs/README.md").write_text("# Docs\n\n" + links, encoding="utf-8")
    if broken:
        (root / "docs/design/old-2026-01.md").write_text("# Old\n", encoding="utf-8")
        (root / "docs/design/ref.md").write_text(
            "# Ref\n\nSee docs/plans/T01-plan.md.\n", encoding="utf-8"
        )
        (root / "web/package.json").write_text('{"version": "0.0.1"}', encoding="utf-8")


def self_test():
    with tempfile.TemporaryDirectory() as tmp:
        good = Path(tmp) / "good"
        write_fixture(good)
        errors, _ = run_checks(good)
        if errors:
            print("self-test failed on the valid fixture:")
            for error in errors:
                print("  " + error)
            return 1

        bad = Path(tmp) / "bad"
        write_fixture(bad, broken=True)
        errors, _ = run_checks(bad)
        text = "\n".join(errors)
        expected = [
            "broken link missing.md",
            "has a date in its name",
            "references a deleted path",
            "web/package.json is 0.0.1",
        ]
        missing = [item for item in expected if item not in text]
        if missing:
            print("self-test did not catch: " + ", ".join(missing))
            for error in errors:
                print("  " + error)
            return 1
    print("self-test ok")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Check documentation consistency")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    root = args.root.resolve()
    errors, version = run_checks(root)
    markdown = sum(1 for _ in iter_files(root, {".md"}))
    if errors:
        for error in errors:
            print("ERROR: " + error)
        print("\ndocs check failed: %d error(s), %d markdown file(s)" % (len(errors), markdown))
        return 1
    print("docs check passed: %d markdown file(s), version %s" % (markdown, version))
    return 0


if __name__ == "__main__":
    sys.exit(main())
