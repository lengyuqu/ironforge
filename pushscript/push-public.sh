#!/usr/bin/env bash
# push-public.sh — 推送到公开 remote，自动剥离私有目录
#
# 用法:
#   ./pushscript/push-public.sh              # 推送到默认公开 remote
#   ./pushscript/push-public.sh <branch>     # 推送到指定分支
#
# 原理: Git 不支持 per-remote .gitignore。
#   每次推送时从 HEAD 创建临时分支 → git rm --cached 剥离私有文件
#   → force-push 到公开 remote → 切回原分支并清理。

set -euo pipefail

# ============================================================
#  配置区 — 按项目修改以下三项
# ============================================================

# 需要从公开 remote 剥离的目录/文件
PRIVATE_PATHS=(
    ".workbuddy/"
    ".claude/"
    ".codex/"
    ".trae/"
    ".qoder/"
    ".codebuddy/"
    "pushscript/"
)

# 公开 remote 名称
PUBLIC_REMOTE="github"

# ============================================================
#  执行逻辑（无需修改）
# ============================================================

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
TARGET_BRANCH="${1:-$CURRENT_BRANCH}"

if ! git remote get-url "$PUBLIC_REMOTE" >/dev/null 2>&1; then
    echo "错误: remote '$PUBLIC_REMOTE' 不存在"
    echo "请先添加: git remote add $PUBLIC_REMOTE <url>"
    exit 1
fi

echo "=== 公开推送: $PUBLIC_REMOTE/$TARGET_BRANCH ==="
echo "剥离: ${PRIVATE_PATHS[*]}"
echo ""

TEMP_BRANCH="__public_sync_$$"
git checkout -b "$TEMP_BRANCH"

cleanup() {
    git checkout -f "$CURRENT_BRANCH" 2>/dev/null || true
    git branch -D "$TEMP_BRANCH" 2>/dev/null || true
}
trap cleanup EXIT

for path in "${PRIVATE_PATHS[@]}"; do
    if git ls-files --error-unmatch "$path" >/dev/null 2>&1; then
        git rm -r --cached --ignore-unmatch "$path" >/dev/null
        echo "  ✓ 剥离: $path"
    else
        echo "  - 跳过(未跟踪): $path"
    fi
done

if ! git diff --cached --quiet; then
    git commit -m "chore: strip private dirs for public mirror" --no-verify
fi

git push --force "$PUBLIC_REMOTE" "$TEMP_BRANCH:$TARGET_BRANCH"
echo ""
echo "✓ 推送完成（工作区文件未受影响）"
