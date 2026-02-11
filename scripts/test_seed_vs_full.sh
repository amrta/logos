#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"
echo "========================================="
echo "  种子系统 vs 完整系统 对比测试"
echo "========================================="
echo ""
for case in "如何优化材料配比？" "什么是量子纠缠？" "为什么天空是蓝色的？" "人工智能会取代人类吗？" "你好"; do
  echo ">>> 测试: $case"
  echo ""
  echo "【种子系统】"
  curl -s -X POST "$WORKER/seed/process" -H "Content-Type: application/json" -d "{\"input\":\"$case\"}" | jq '{rule: .seed_rule, confidence: .confidence, source: .source, learned: .learned_count}'
  echo ""
  echo "---"
done
echo ""
echo "========================================="
echo "  测试完成"
echo "========================================="
