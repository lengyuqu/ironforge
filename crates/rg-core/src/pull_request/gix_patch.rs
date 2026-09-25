//! gix-native unified patch generation — blob-diff 对拍 spike（CLAUDE.md 技术债表「blob-diff patch」项）。
//!
//! 目标：与 `git -c core.quotePath=false diff --no-ext-diff --find-renames <base>...<head>`
//! 的输出**字节级一致**，使 `compute_same_repo_diff` / `compute_cross_repo_diff` 可以去掉
//! CLI 网关调用（`pull_request/service.rs` 两处 `TODO(gix)`）。
//!
//! 已处理的 git 格式细节：
//! - 三点语义（`base...head` = merge-base(base, head) → head 的 diff）
//! - rename/rewrite 检测（`Rewrites::default()` ≈ `--find-renames` 默认 50% 阈值 + limit 1000）
//! - 文件头：`diff --git` / `old mode` / `new mode` / `new file mode` / `deleted file mode` /
//!   `similarity index` / `rename from|to` / `index <abbrev>..<abbrev>[ <mode>]`
//! - 100% 纯 rename 不带 index/---/+++ 行
//! - hunk 头 git 省略规则：len == 1 省略 `,len`；len == 0 时 start 回退一行（`-0,0` / `+4,0`）
//! - funcname 上下文（`@@ ... @@ fn beta() {`）：从首个上下文行的前一行向下扫，
//!   默认规则 = 首字节为 ASCII 字母 / `_` / `$`（xdiff `def_ff`，实测钉死）
//! - `\ No newline at end of file` 标记（按新旧镜像各自的光标位置判定，而非行内容）
//! - 二进制文件：`Binary files a/x and b/x differ`（无 ---/+++）
//!
//! 已知边界（fixture 未覆盖，与 git 语义待验证）：
//! - submodule（160000）条目：git 输出 `-Subproject commit` 形式，此处不支持
//! - 深层目录与文件的字典序交叉（`a-b` vs `a/b`）排序差异
//! - rename 相似度百分比的舍入可能相差 1%

use std::io;
use std::path::Path;

use anyhow::{Context as _, bail};
use gix::bstr::{BStr, BString, ByteSlice};
use gix::diff::blob::platform::prepare_diff::Operation;
use gix::diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix::diff::blob::UnifiedDiff;
use gix::object::tree::diff::{Change, ChangeDetached};
use gix::prelude::TreeDiffChangeExt as _;

/// 生成与 `git diff --no-ext-diff --find-renames <base>...<head>` 字节级一致的 unified patch。
pub fn unified_patch(repo_path: &Path, base_ref: &str, head_ref: &str) -> anyhow::Result<Vec<u8>> {
    let repo = gix::open(repo_path)
        .with_context(|| format!("failed to open repository: {}", repo_path.display()))?;
    let base_id = repo
        .rev_parse_single(base_ref)
        .with_context(|| format!("ref not found: {base_ref}"))?;
    let head_id = repo
        .rev_parse_single(head_ref)
        .with_context(|| format!("ref not found: {head_ref}"))?;

    // `<base>...<head>` 语义：merge-base(base, head) → head
    let merge_base = repo.merge_base(base_id.detach(), head_id.detach())?;
    let old_tree = merge_base.object()?.peel_to_tree()?;
    let new_tree = head_id.object()?.peel_to_tree()?;

    let mut out: Vec<u8> = Vec::new();
    let mut resource_cache = repo.diff_resource_cache(
        gix::diff::blob::pipeline::Mode::ToGit,
        gix::diff::blob::pipeline::WorktreeRoots::default(),
    )?;
    // 等价于 CLI 的 `--no-ext-diff`
    resource_cache.options.skip_internal_diff_if_external_is_configured = false;

    let mut platform = old_tree.changes()?;
    platform.options(|opts| {
        // 等价于 CLI 的 `--find-renames`（默认 50% 相似度阈值、limit 1000）
        opts.track_rewrites(Some(gix::diff::Rewrites::default()));
    });

    // git 按全路径字节序输出；gix tree-diff 按目录层级遍历，先收集再排序对齐
    let mut changes: Vec<(BString, ChangeDetached)> = Vec::new();
    platform
        .for_each_to_obtain_tree(
            &new_tree,
            |change| -> anyhow::Result<std::ops::ControlFlow<()>> {
                let key = change.location().to_owned();
                changes.push((key, change.detach()));
                Ok(std::ops::ControlFlow::Continue(()))
            },
        )
        .map_err(|e| anyhow::anyhow!("tree-diff failed: {e:?}"))?;
    changes.sort_by(|a, b| a.0.cmp(&b.0));

    for (_, detached) in &changes {
        let change = detached.attach(&repo, &repo);
        render_change(&change, &mut resource_cache, &mut out)?;
        resource_cache.clear_resource_cache_keep_allocation();
    }

    Ok(out)
}

// ── 变更渲染 ────────────────────────────────────────────────────────────

fn render_change(
    change: &Change,
    cache: &mut gix::diff::blob::Platform,
    out: &mut Vec<u8>,
) -> anyhow::Result<()> {
    // 目录条目由其内部文件体现，`git diff` 不单独输出 Tree 变更
    let is_tree = match change {
        Change::Addition { entry_mode, .. } | Change::Deletion { entry_mode, .. } => {
            entry_mode.is_tree()
        }
        Change::Modification {
            entry_mode,
            previous_entry_mode,
            ..
        } => entry_mode.is_tree() || previous_entry_mode.is_tree(),
        Change::Rewrite { .. } => false,
    };
    if is_tree {
        return Ok(());
    }

    match change {
        Change::Addition {
            location,
            entry_mode,
            id,
            ..
        } => {
            let p = location.as_bstr();
            push_diff_git_line(out, p, p);
            push_str(out, &format!("new file mode {:o}\n", entry_mode.value()));
            let abbrev = id.shorten().context("failed to abbreviate id")?;
            push_str(out, &format!("index {}..{abbrev}\n", zeros(abbrev.to_string().len())));
            emit_body(change, cache, out, "/dev/null", &format!("b/{}", p.to_str_lossy()))?;
        }
        Change::Deletion {
            location,
            entry_mode,
            id,
            ..
        } => {
            let p = location.as_bstr();
            push_diff_git_line(out, p, p);
            push_str(out, &format!("deleted file mode {:o}\n", entry_mode.value()));
            let abbrev = id.shorten().context("failed to abbreviate id")?;
            push_str(out, &format!("index {abbrev}..{}\n", zeros(abbrev.to_string().len())));
            emit_body(change, cache, out, &format!("a/{}", p.to_str_lossy()), "/dev/null")?;
        }
        Change::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
            ..
        } => {
            let p = location.as_bstr();
            push_diff_git_line(out, p, p);
            if previous_entry_mode != entry_mode {
                push_str(
                    out,
                    &format!(
                        "old mode {:o}\nnew mode {:o}\n",
                        previous_entry_mode.value(),
                        entry_mode.value()
                    ),
                );
            }
            let old_abbrev = previous_id.shorten().context("failed to abbreviate id")?;
            let new_abbrev = id.shorten().context("failed to abbreviate id")?;
            let mode_suffix = if previous_entry_mode == entry_mode {
                format!(" {:o}", entry_mode.value())
            } else {
                String::new()
            };
            push_str(
                out,
                &format!("index {old_abbrev}..{new_abbrev}{mode_suffix}\n"),
            );
            emit_body(
                change,
                cache,
                out,
                &format!("a/{}", p.to_str_lossy()),
                &format!("b/{}", p.to_str_lossy()),
            )?;
        }
        Change::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            diff,
            entry_mode,
            id,
            location,
            ..
        } => {
            let src = source_location.as_bstr();
            let dst = location.as_bstr();
            push_diff_git_line(out, src, dst);

            let similarity = diff.as_ref().map_or(100.0, |s| s.similarity * 100.0);
            // git 的 similarity index 为整数截断（非四舍五入），实测 92.5 → 92
            push_str(
                out,
                &format!("similarity index {}%\n", similarity.floor().clamp(0.0, 100.0)),
            );
            push_str(out, &format!("rename from {}\n", src.to_str_lossy()));
            push_str(out, &format!("rename to {}\n", dst.to_str_lossy()));

            if source_entry_mode != entry_mode {
                push_str(
                    out,
                    &format!(
                        "old mode {:o}\nnew mode {:o}\n",
                        source_entry_mode.value(),
                        entry_mode.value()
                    ),
                );
            }
            // 纯 rename（source_id == id）：git 不输出 index/---/+++ 行
            if source_id.detach() == id.detach() {
                return Ok(());
            }
            let old_abbrev = source_id.shorten().context("failed to abbreviate id")?;
            let new_abbrev = id.shorten().context("failed to abbreviate id")?;
            let mode_suffix = if source_entry_mode == entry_mode {
                format!(" {:o}", entry_mode.value())
            } else {
                String::new()
            };
            push_str(
                out,
                &format!("index {old_abbrev}..{new_abbrev}{mode_suffix}\n"),
            );
            emit_body(
                change,
                cache,
                out,
                &format!("a/{}", src.to_str_lossy()),
                &format!("b/{}", dst.to_str_lossy()),
            )?;
        }
    }
    Ok(())
}

/// 渲染 `---`/`+++` 行与 hunks。内容为空（无 hunks）时两者都不输出；二进制时输出 Binary files 行。
fn emit_body(
    change: &Change,
    cache: &mut gix::diff::blob::Platform,
    out: &mut Vec<u8>,
    a_label: &str,
    b_label: &str,
) -> anyhow::Result<()> {
    let mut hunks: Vec<u8> = Vec::new();
    let platform = change.diff(cache)?;
    let prep = platform.resource_cache.prepare_diff()?;
    match prep.operation {
        Operation::InternalDiff { algorithm } => {
            // git 语义：行终止符属于行记录（"无换行末行" != "有换行同行"）。
            // 不能用 `prep.interned_input()`——它刻意剥离终止符，行为与 git 相反。
            let input =
                gix::diff::blob::InternedInput::new(prep.old.intern_source(), prep.new.intern_source());
            let diff = gix::diff::blob::diff_with_slider_heuristics(algorithm, &input);
            let renderer = GitHunkRenderer {
                input: &input,
                out: &mut hunks,
            };
            UnifiedDiff::new(&diff, &input, renderer, ContextSize::symmetrical(3)).consume()?;
        }
        Operation::SourceOrDestinationIsBinary => {
            push_str(out, &format!("Binary files {a_label} and {b_label} differ\n"));
            return Ok(());
        }
        Operation::ExternalCommand { command } => {
            bail!("external diff driver configured: {}", command.to_str_lossy())
        }
    }
    if !hunks.is_empty() {
        push_str(out, &format!("--- {a_label}\n"));
        push_str(out, &format!("+++ {b_label}\n"));
        out.extend_from_slice(&hunks);
    }
    Ok(())
}

// ── hunk 渲染（git 格式） ───────────────────────────────────────────────

/// 实现 git 的 hunk 渲染规则，供 [`UnifiedDiff`] 回调。
struct GitHunkRenderer<'a> {
    input: &'a gix::diff::blob::InternedInput<&'a [u8]>,
    out: &'a mut Vec<u8>,
}

impl ConsumeHunk for GitHunkRenderer<'_> {
    type Out = ();

    fn consume_hunk(&mut self, header: HunkHeader, lines: &[(DiffLineKind, &[u8])]) -> io::Result<()> {
        // git 省略规则：len == 1 省略 ",len"；len == 0 时 start 回退一行
        let before = fmt_range(header.before_hunk_start, header.before_hunk_len);
        let after = fmt_range(header.after_hunk_start, header.after_hunk_len);
        self.out
            .extend_from_slice(format!("@@ -{before} +{after} @@").as_bytes());
        if let Some(func) = self
            .find_func_line(header.before_hunk_start)
            .map(|f| f.to_vec())
        {
            self.out.push(b' ');
            self.out.extend_from_slice(&func);
        }
        self.out.push(b'\n');

        // token 保留行终止符：无尾换行的行内容不含 '\n'，即触发 marker
        for &(kind, content) in lines {
            self.out.push(kind.to_prefix() as u8);
            self.out.extend_from_slice(content);
            if content.ends_with(b"\n") {
                continue;
            }
            self.out.push(b'\n');
            self.out
                .extend_from_slice(b"\\ No newline at end of file\n");
        }
        Ok(())
    }

    fn finish(self) {}
}

impl GitHunkRenderer<'_> {
    /// git funcname 上下文：从首个上下文行的**前一行**（0-based `display_start - 2`）向下扫，
    /// 默认规则（xdiff `def_ff`）= 首字节为 ASCII 字母 / `_` / `$`。
    /// 实测钉死：首个上下文行本身不参与匹配（`@@ -5,7 +5,7 @@ int unrelated() {` 用例）。
    fn find_func_line(&self, display_start: u32) -> Option<&[u8]> {
        let start = (display_start as usize).checked_sub(2)?;
        for l in (0..=start).rev() {
            let token = self.input.before[l];
            let line = self.input.interner[token];
            if is_func_line(line) {
                // token 含行终止符，funcname 输出前剥掉（调用方统一补 '\n'）
                let mut end = line.len();
                while end > 0 && (line[end - 1] == b'\n' || line[end - 1] == b'\r') {
                    end -= 1;
                }
                return Some(&line[..end]);
            }
        }
        None
    }
}

fn is_func_line(line: &[u8]) -> bool {
    matches!(line.first(), Some(&c) if matches!(c, b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
}

fn fmt_range(start: u32, len: u32) -> String {
    let start = if len == 0 { start.saturating_sub(1) } else { start };
    if len == 1 {
        format!("{start}")
    } else {
        format!("{start},{len}")
    }
}

// ── 小工具 ─────────────────────────────────────────────────────────────

/// `diff --git a/<p> b/<p>` 行（quotePath=false 语义：原始字节输出）
fn push_diff_git_line(out: &mut Vec<u8>, a: &BStr, b: &BStr) {
    out.extend_from_slice(b"diff --git a/");
    out.extend_from_slice(a);
    out.extend_from_slice(b" b/");
    out.extend_from_slice(b);
    out.push(b'\n');
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn zeros(n: usize) -> String {
    "0".repeat(n)
}

// ── 对拍测试 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod parity_tests {
    use super::*;
    use rg_git::cli_gateway::global_gateway;

    /// 在临时目录构建 fixture 仓库（全部经 git CLI 造数据），返回 (repo_path, base_ref, head_ref)。
    fn fixture_repo() -> anyhow::Result<(tempfile::TempDir, std::path::PathBuf)> {
        let dir = tempfile::tempdir()?;
        let repo = dir.path().join("fixture");
        let git = global_gateway().as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;

        let run = |args: &[&str], cwd: Option<&Path>| -> anyhow::Result<()> {
            git.run_or_bail(args, cwd)
                .with_context(|| format!("git {args:?}"))?;
            Ok(())
        };
        let write = |rel: &str, content: &[u8]| -> anyhow::Result<()> {
            let p = repo.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, content)?;
            Ok(())
        };

        run(&["init", "-b", "main", &repo.to_string_lossy()], None)?;
        // 本机全局可能 core.autocrlf=true，仓库级强制关闭以保证字节级可比
        run(&["config", "core.autocrlf", "false"], Some(&repo))?;
        run(&["config", "user.email", "fixture@example.com"], Some(&repo))?;
        run(&["config", "user.name", "Fixture"], Some(&repo))?;
        run(&["config", "commit.gpgsign", "false"], Some(&repo))?;

        // ── base 提交 ──
        let mut src = String::new();
        src.push_str("fn alpha() {\n");
        for i in 1..=12 {
            src.push_str(&format!("    let a{i} = {i};\n"));
        }
        src.push_str("    println!(\"alpha done\");\n}\n\n");
        src.push_str("fn beta() {\n");
        for i in 1..=12 {
            src.push_str(&format!("    let b{i} = {i};\n"));
        }
        src.push_str("    println!(\"beta done\");\n}\n\n");
        src.push_str("fn gamma() {\n");
        for i in 1..=12 {
            src.push_str(&format!("    let c{i} = {i};\n"));
        }
        src.push_str("    println!(\"gamma done\");\n}\n");
        write("src/service.rs", src.as_bytes())?;

        write(
            "docs/readme.md",
            b"line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\n",
        )?;
        write("nonl.txt", b"first\nlast line no newline")?;
        write("nonl2.txt", b"alpha\nbeta\ngamma")?;
        write("bin/run.sh", b"#!/bin/sh\necho run\n")?;
        write("bin/other.sh", b"#!/bin/sh\necho other\n")?;
        write("img/logo.bin", &[0x00u8, 0x01, 0x02, 0xFF, 0xFE, 0x7F, 0x80])?;
        write("empty.txt", b"")?;
        write("old_name.txt", b"rename me exactly\n")?;
        write("moved/edited.txt", &{
            let mut v = Vec::new();
            for i in 1..=20 {
                v.extend_from_slice(format!("content line {i}\n").as_bytes());
            }
            v
        })?;
        write("del.txt", b"delete me\nline two\n")?;

        run(&["add", "-A"], Some(&repo))?;
        run(&["commit", "-m", "base"], Some(&repo))?;
        run(&["checkout", "-b", "feature"], Some(&repo))?;

        // ── head 提交（feature） ──
        // 1) src/service.rs：beta() 内单行改动（hunk len==1）+ alpha() 内改动（相距足够远 → 多 hunk）
        let src2 = src
            .replacen("    let b5 = 5;", "    let b5 = 55;", 1)
            .replacen("    let a3 = 3;", "    let a3 = 33;\n    let a3b = 333;", 1);
        assert_ne!(src, src2);
        write("src/service.rs", src2.as_bytes())?;

        // 2) docs/readme.md：在 EOF 附近追加（上下文裁剪）
        write(
            "docs/readme.md",
            b"line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9\nline 10\nline 11\nline 12\nline 13\nline 14\nline 15\nline 16\nline 17\nline 18\nline 19\nline 20\nline 21\nline 22\n",
        )?;

        // 3) nonl.txt：改末行且两侧都无尾换行（双 marker）
        write("nonl.txt", b"first\nlast line CHANGED")?;
        // 4) nonl2.txt：无尾换行文件末尾追加行（interned_input 剥终止符的特殊用例）
        write("nonl2.txt", b"alpha\nbeta\ngamma\ndelta\nepsilon")?;

        // 5) 仅 chmod +x（mode-only change）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(repo.join("bin/run.sh"), std::fs::Permissions::from_mode(0o755))?;
        }
        // 6) chmod +x 且内容变化
        write("bin/other.sh", b"#!/bin/sh\necho other changed\n")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(repo.join("bin/other.sh"), std::fs::Permissions::from_mode(0o755))?;
        }

        // 7) 二进制文件变化
        write("img/logo.bin", &[0x00u8, 0x01, 0x03, 0xFF, 0x00, 0xDE, 0xAD, 0xBE, 0xEF])?;

        // 8) 空文件 → 有内容
        write("empty.txt", b"now has content\nsecond line\n")?;

        // 9) 精确 rename（100%）
        run(&["mv", "old_name.txt", "new_name.txt"], Some(&repo))?;
        // 10) rename + 编辑
        run(&["mv", "moved/edited.txt", "moved/renamed_edited.txt"], Some(&repo))?;
        let mut edited = Vec::new();
        for i in 1..=20 {
            let line = if i == 10 { format!("content line {i} EDITED\n") } else { format!("content line {i}\n") };
            edited.extend_from_slice(line.as_bytes());
        }
        write("moved/renamed_edited.txt", &edited)?;

        // 11) 删除文件
        std::fs::remove_file(repo.join("del.txt"))?;

        run(&["add", "-A"], Some(&repo))?;
        run(&["commit", "-m", "head"], Some(&repo))?;

        Ok((dir, repo))
    }

    /// CLI 参考实现（与 `compute_same_repo_diff` 的调用方式一致）
    fn cli_patch(repo: &Path, range: &str) -> anyhow::Result<Vec<u8>> {
        let git = global_gateway().as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;
        let output = git.run(
            &[
                "-c",
                "core.quotePath=false",
                "-c",
                "color.ui=false",
                "diff",
                "--no-ext-diff",
                "--find-renames",
                range,
            ],
            Some(repo),
        )?;
        output.ensure_success()?;
        Ok(output.stdout)
    }

    #[test]
    fn gix_patch_matches_git_cli_byte_for_byte() {
        let (_dir, repo) = fixture_repo().expect("fixture repo");

        let gix_out = unified_patch(&repo, "main", "feature").expect("gix patch");
        let cli_out = cli_patch(&repo, "main...feature").expect("cli patch");

        if gix_out != cli_out {
            let g = String::from_utf8_lossy(&gix_out);
            let c = String::from_utf8_lossy(&cli_out);
            panic!(
                "gix patch != cli patch\n--- gix ---\n{g}\n--- cli ---\n{c}\n--- diff detail ---\n{}",
                first_divergence(&gix_out, &cli_out)
            );
        }
    }

    fn first_divergence(a: &[u8], b: &[u8]) -> String {
        let la: Vec<&[u8]> = a.split(|&c| c == b'\n').collect();
        let lb: Vec<&[u8]> = b.split(|&c| c == b'\n').collect();
        for i in 0..la.len().max(lb.len()) {
            match (la.get(i), lb.get(i)) {
                (Some(x), Some(y)) if x == y => {}
                (x, y) => {
                    return format!(
                        "first differing line {}:\n gix: {:?}\n cli: {:?}",
                        i + 1,
                        x.map(|v| String::from_utf8_lossy(v)),
                        y.map(|v| String::from_utf8_lossy(v))
                    );
                }
            }
        }
        "no divergence found".into()
    }
}
