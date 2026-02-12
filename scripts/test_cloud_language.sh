#!/usr/bin/env bash
W="${WORKER_URL:-https://logos-gateway.amrta.workers.dev}"
queries=("三原色是什么？" "描述原子的结构。" "如何减少空气污染？" "保持健康的三个提示。" "什么是量子纠缠")
for q in "${queries[@]}"; do
  r=$(curl -s -X POST "$W/execute" -H "Content-Type: application/json" -d "{\"command\":$(echo "$q" | jq -Rs .),\"mode\":\"natural\"}")
  hit=$(echo "$r" | jq -r '.reply // .error // empty' | head -c 120)
  echo "[$q] -> ${hit}..."
done
