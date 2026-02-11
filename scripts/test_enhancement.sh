#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"
echo "========================================="
echo "  递归增强测试"
echo "========================================="
echo ""
for case in "如何优化深度学习模型的性能？" "什么是区块链的共识机制？" "为什么机器学习需要大量数据？"; do
  echo ">>> 输入: $case"
  echo ""
  curl -s -X POST "$WORKER/enhance" -H "Content-Type: application/json" -d "{\"input\":\"$case\"}" | jq '{layers_used: .layers_used, layer_details: .layer_details, output_preview: (.final_output | .[0:100])}'
  echo ""
  echo "---"
  echo ""
done
echo "========================================="
echo "  增强统计"
echo "========================================="
curl -s "$WORKER/enhance/stats" | jq .
