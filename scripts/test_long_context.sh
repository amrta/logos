#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"
echo "========================================="
echo "  长上下文处理测试"
echo "========================================="
echo ""
LONG_TEXT="深度学习是机器学习的一个分支。"
for i in $(seq 1 100); do
  LONG_TEXT="${LONG_TEXT}它使用多层神经网络来学习数据的表示。深度学习在图像识别、自然语言处理等领域取得了巨大成功。"
done
echo "生成测试文本：${#LONG_TEXT} 字符"
echo ""
echo ">>> 测试1: 短文本（<4K）"
curl -s -X POST "$WORKER/expand" -H "Content-Type: application/json" -d '{"input":"什么是深度学习？"}' | jq '{input_length: .input_length, segments: .segments_count, truncated: .truncated}'
echo ""
echo ">>> 测试2: 中文本（4-8K）"
SHORT_TEXT="${LONG_TEXT:0:5000}"
curl -s -X POST "$WORKER/expand" -H "Content-Type: application/json" -d "{\"input\":\"$SHORT_TEXT\"}" | jq '{input_length: .input_length, segments: .segments_count, truncated: .truncated, output_preview: (.output | .[0:100])}'
echo ""
echo ">>> 测试3: 长文本（>8K）"
curl -s -X POST "$WORKER/expand" -H "Content-Type: application/json" -d "{\"input\":\"$LONG_TEXT\"}" | jq '{input_length: .input_length, segments: .segments_count, truncated: .truncated, max_size: .max_size}'
echo ""
