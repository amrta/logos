#!/bin/bash

WORKER="${LOGOS_WORKER_URL:-https://logos-gateway.amrta.workers.dev}"

echo "========================================="
echo "  LOGOS 学习闭环测试"
echo "========================================="
echo ""

echo ">>> 测试1: 提交安全的候选逻辑"
curl -s -X POST "$WORKER/evolution/submit" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "reasoning_pouch",
    "description": "优化材料配比算法",
    "code": "fn optimize_ratio(a: f32, b: f32) -> f32 { (a + b) / 2.0 }"
  }' | jq .
echo ""

echo ">>> 测试2: 提交不安全的候选逻辑（应该被拒绝）"
curl -s -X POST "$WORKER/evolution/submit" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "malicious",
    "description": "尝试修改frozen层",
    "code": "use frozen::bedrock; fn hack() { bedrock::SYSTEM_NAME = \"hacked\"; }"
  }' | jq .
echo ""

echo ">>> 测试3: reasoning反馈专业术语给language"
curl -s -X POST "$WORKER/pouch/feedback" \
  -H "Content-Type: application/json" \
  -d '{
    "from_pouch": "reasoning",
    "to_pouch": "language",
    "feedback_type": "terminology",
    "content": "材料配比优化",
    "confidence": 0.9
  }' | jq .
echo ""

sleep 2

echo ">>> 测试4: 学习新模式"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{
    "input": "如何优化3D打印的材料配比？",
    "output": "可以使用黄金比例算法，将材料A和材料B按照1.618:1的比例混合，这样可以获得最佳的强度和韧性平衡。",
    "source": "material_pouch"
  }' | jq .
echo ""

echo ">>> 测试5: 学习统计"
curl -s "$WORKER/learn/stats" | jq .
echo ""

echo ">>> 测试6: 演化统计"
curl -s "$WORKER/evolution/stats/validation" | jq .
echo ""

echo ">>> 测试7: 最近学习的模式（前5条）"
curl -s "$WORKER/learn/patterns?limit=5" | jq '.[] | {keywords: .pattern_keywords, source: .source, confidence: .confidence}'
echo ""

echo ">>> 测试8: 演化候选（前3条）"
curl -s "$WORKER/evolution/list" | jq '.[0:3] | .[] | {id: .id, source: .source, status: .status, safety: .safety_score}'
echo ""

echo "========================================="
echo "  测试完成"
echo "========================================="
