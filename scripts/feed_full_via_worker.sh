#!/usr/bin/env bash
# 通过 CF Worker 把整份 JSONL 按批喂到 Logos。需在 wrangler 配置 LOGOS_BACKEND + LOGOS_JSONL_URL，
# 或本脚本传 BACKEND 与 JSONL_URL 环境变量（或作为参数）。
set -e
WORKER_URL="${WORKER_URL:-https://logos-gateway.amrta.workers.dev}"
BACKEND="${BACKEND:-}"
JSONL_URL="${JSONL_URL:-}"
OFFSET=0
LIMIT="${LIMIT:-500}"

if [ -n "$1" ]; then BACKEND="$1"; fi
if [ -n "$2" ]; then JSONL_URL="$2"; fi

while true; do
  if [ -n "$BACKEND" ] && [ -n "$JSONL_URL" ]; then
    BODY="{\"backend\":\"$BACKEND\",\"url\":\"$JSONL_URL\",\"limit\":$LIMIT,\"offset\":$OFFSET}"
  else
    BODY="{\"offset\":$OFFSET,\"limit\":$LIMIT}"
  fi
  R=$(curl -s -X POST "$WORKER_URL/feed" -H "Content-Type: application/json" -d "$BODY")
  echo "$R"
  echo "$R" | grep -q '"done":true' && { echo "Done."; exit 0; }
  NEXT=$(echo "$R" | sed -n 's/.*"next_offset":\([0-9]*\).*/\1/p')
  [ -z "$NEXT" ] || [ "$NEXT" = "$OFFSET" ] && { echo "No progress or error."; exit 1; }
  OFFSET=$NEXT
  sleep 2
done
