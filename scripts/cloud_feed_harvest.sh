#!/usr/bin/env bash
# 云端一次性投喂：循环调用 Worker /harvest，每次从 HF 拉 50 条写入 D1。
# 需 Worker 已部署（含 /harvest 路由）且有 D1。若返回 Unknown endpoint，请先 wrangler deploy。
# 可替代：node scripts/cloud_feed_hf.mjs alpaca-zh  直接走 /feed/language-upload，无需 deploy。
# 用法: ./cloud_feed_harvest.sh [轮数]
set -e
WORKER_URL="${WORKER_URL:-https://logos-gateway.amrta.workers.dev}"
ROUNDS="${1:-50}"
for i in $(seq 1 "$ROUNDS"); do
  R=$(curl -s -X POST "$WORKER_URL/harvest")
  echo "[$i/$ROUNDS] $R"
  if echo "$R" | grep -q '"exhausted":true'; then
    echo "harvest exhausted"
    break
  fi
  sleep 1
done
