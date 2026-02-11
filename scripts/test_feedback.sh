#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"

echo "========================================="
echo "  尿袋反馈机制测试"
echo "========================================="
echo ""

echo ">>> 第1步：reasoning反馈专业术语给language"
curl -s -X POST "$WORKER/pouch/feedback" \
  -H "Content-Type: application/json" \
  -d '{"from_pouch": "reasoning", "to_pouch": "language", "feedback_type": "terminology", "content": "深度学习优化技巧：学习率调度、梯度裁剪、早停法", "confidence": 0.9}' | jq .

sleep 2

echo ""
echo ">>> 第2步：material反馈给language"
curl -s -X POST "$WORKER/pouch/feedback" \
  -H "Content-Type: application/json" \
  -d '{"from_pouch": "material", "to_pouch": "language", "feedback_type": "terminology", "content": "复合材料配比：碳纤维70%、环氧树脂30%", "confidence": 0.85}' | jq .

sleep 2

echo ""
echo ">>> 第3步：查看language收到的反馈"
curl -s "$WORKER/pouch/language/feedback" | jq '.[] | {from: .from_pouch, content: .content, type: .feedback_type, confidence: .confidence}'

echo ""
echo ">>> 第4步：学习统计（应该因反馈而增加）"
curl -s "$WORKER/learn/stats" | jq .

echo ""
echo ">>> 第5步：测试反馈是否触发了学习"
curl -s "$WORKER/learn/patterns?limit=10" | jq '.[] | select(.source == "reasoning" or .source == "material") | {keywords: .pattern_keywords, source: .source}'

echo ""
echo "========================================="
echo "  反馈测试完成"
echo "========================================="
