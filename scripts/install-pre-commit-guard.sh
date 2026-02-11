#!/usr/bin/env bash
# 安装 pre-commit 钩子：若 origin 被改则拦 commit 并提示恢复命令。执行一次即可。
set -e
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXPECTED_FILE="$REPO_ROOT/config/expected-git-origin.txt"
HOOK="$REPO_ROOT/.git/hooks/pre-commit"
if [ ! -f "$EXPECTED_FILE" ]; then
  echo "缺少 config/expected-git-origin.txt，请填入你正确的 origin URL（一行）"
  exit 1
fi
mkdir -p "$(dirname "$HOOK")"
cat > "$HOOK" << 'HOOK_SCRIPT'
#!/bin/sh
REPO_ROOT="$(git rev-parse --show-toplevel)"
EXPECTED=$(cat "$REPO_ROOT/config/expected-git-origin.txt" 2>/dev/null | tr -d '\n\r')
CURRENT=$(git remote get-url origin 2>/dev/null)
if [ -z "$EXPECTED" ]; then exit 0; fi
if [ "$CURRENT" != "$EXPECTED" ]; then
  echo "origin 已被修改，当前: $CURRENT"
  echo "恢复: git remote set-url origin $EXPECTED"
  exit 1
fi
exit 0
HOOK_SCRIPT
chmod +x "$HOOK"
echo "已安装 .git/hooks/pre-commit：若 origin 与 config/expected-git-origin.txt 不一致将拒绝 commit 并提示恢复命令。"
