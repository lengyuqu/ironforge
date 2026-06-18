#!/usr/bin/env bash
# push-github.sh — 推送到 GitHub，自动剥离 AI 工具目录
#
# 用法:
#   ./scripts/push-github.sh              # 推送当前分支到 github/main
#   ./scripts/push-github.sh feature-x    # 推送当前分支到 github/指定分支
#
# 工作原理:
#   1. 从当前 HEAD 创建临时分支
#   2. 在临时分支上 git rm --cached 剥离工具目录（文件保留在工作区）
#   3. force-push 到 GitHub
#   4. 切回原分支，删除临时分支
#
# git233 (origin) 不受影响，工具目录继续跟踪。

set -euo pipefail

# 需要从 GitHub 剥离的目录（AI 工具 + 本脚本自身）
# 这些目录仅在 git233 (origin) 跟踪，不推送到 GitHub
TOOL_DIRS=(
    ".workbuddy"
    ".claude"
    ".codex"
    ".trae"
    ".qoder"
    ".codebuddy"
    "scripts"
)

GITHUB_REMOTE="github"
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
TARGET_BRANCH="${1:-$CURRENT_BRANCH}"

# 检查 github remote 是否存在
if ! git remote get-url "$GITHUB_REMOTE" >/dev/null 2>&1; then
    echo "错误: remote '$GITHUB_REMOTE' 不存在。"
    echo "请先添加: git remote add github <github-repo-url>"
    exit 1
fi

echo "=== GitHub 同步推送 ==="
echo "源分支:   $CURRENT_BRANCH"
echo "目标分支: $GITHUB_REMOTE/$TARGET_BRANCH"
echo "剥离目录: ${TOOL_DIRS[*]}（含 scripts/ 本身）"
echo ""

# 创建临时分支
TEMP_BRANCH="__github_sync_$$"
git checkout -b "$TEMP_BRANCH"

# 确保退出时切回原分支并清理临时分支
cleanup() {
    git checkout "$CURRENT_BRANCH" 2>/dev/null || true
    git branch -D "$TEMP_BRANCH" 2>/dev/null || true
}
trap cleanup EXIT

# 剥离工具目录（仅从索引移除，工作区文件保留）
for dir in "${TOOL_DIRS[@]}"; do
    if git ls-files --error-unmatch "$dir" >/dev/null 2>&1 || \
       git ls-files --error-unmatch "$dir/" >/dev/null 2>&1; then
        git rm -r --cached --ignore-unmatch "$dir" >/dev/null
        echo "  已剥离: $dir/"
    else
        echo "  跳过(未跟踪): $dir/"
    fi
done

# 只有在有变更时才提交
if ! git diff --cached --quiet; then
    git commit -m "chore: strip AI tool dirs for GitHub sync" --no-verify
    echo ""
    echo "已创建剥离提交。"
else
    echo ""
    echo "无需剥离（工具目录未被跟踪）。"
fi

# Force-push 到 GitHub
echo ""
echo "推送到 $GITHUB_REMOTE/$TARGET_BRANCH ..."
git push --force "$GITHUB_REMOTE" "$TEMP_BRANCH:$TARGET_BRANCH"

echo ""
echo "✓ GitHub 同步完成。"
