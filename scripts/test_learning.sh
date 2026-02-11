#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"

echo "========================================="
echo "  学习机制完整测试"
echo "========================================="
echo ""

echo ">>> 第1步：提交5条学习材料"
echo ""

echo "材料1: 如何优化深度学习模型"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{"input": "如何优化深度学习模型", "output": "可以通过以下方法优化：1.调整学习率 2.使用批归一化 3.数据增强 4.模型剪枝 5.知识蒸馏", "source": "reasoning"}' | jq '{learned: .learned, confidence: .confidence, keywords: .keywords}'

echo ""
echo "材料2: 什么是卷积神经网络"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{"input": "什么是卷积神经网络", "output": "CNN是一种专门用于处理图像数据的深度学习架构，通过卷积层提取空间特征", "source": "reasoning"}' | jq '{learned: .learned, confidence: .confidence}'

echo ""
echo "材料3: 为什么需要正则化"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{"input": "为什么需要正则化", "output": "正则化可以防止模型过拟合，提高泛化能力，常用方法包括L1/L2正则化和Dropout", "source": "reasoning"}' | jq '{learned: .learned, confidence: .confidence}'

echo ""
echo "材料4: 如何选择优化器"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{"input": "如何选择优化器", "output": "Adam适合大多数场景，SGD+Momentum适合需要精细调参的情况，RMSprop适合RNN", "source": "reasoning"}' | jq '{learned: .learned, confidence: .confidence}'

echo ""
echo "材料5: 什么是迁移学习"
curl -s -X POST "$WORKER/learn" \
  -H "Content-Type: application/json" \
  -d '{"input": "什么是迁移学习", "output": "迁移学习是利用预训练模型在新任务上进行微调的技术，可以显著减少训练时间和数据需求", "source": "reasoning"}' | jq '{learned: .learned, confidence: .confidence}'

echo ""
echo ">>> 第2步：查看学习统计"
curl -s "$WORKER/learn/stats" | jq .

echo ""
echo ">>> 第3步：查看学到的模式（前5条）"
curl -s "$WORKER/learn/patterns?limit=5" | jq '.[] | {keywords: .pattern_keywords, confidence: .confidence, source: .source, status: .status}'

echo ""
echo ">>> 第4步：测试种子+学习增强"
curl -s -X POST "$WORKER/seed/process" \
  -H "Content-Type: application/json" \
  -d '{"input":"如何优化深度学习"}' | jq '{seed_rule: .seed_rule, confidence: .confidence, source: .source, learned_count: .learned_count}'

echo ""
echo ">>> 第5步：测试递归增强（应该有learned增强）"
curl -s -X POST "$WORKER/enhance" \
  -H "Content-Type: application/json" \
  -d '{"input":"如何优化深度学习模型"}' | jq '{layers_used: .layers_used, layer_0_confidence: .layer_details[0].confidence, layer_1_confidence: .layer_details[1].confidence, layer_2_confidence: .layer_details[2].confidence, output_length: (.final_output | length)}'

echo ""
echo ">>> 第6步：对比学习前后"
echo "学习前（新问题）："
curl -s -X POST "$WORKER/seed/process" \
  -H "Content-Type: application/json" \
  -d '{"input":"如何提升模型准确率"}' | jq '{learned_count: .learned_count, confidence: .confidence}'

echo ""
echo "学习后（已学问题）："
curl -s -X POST "$WORKER/seed/process" \
  -H "Content-Type: application/json" \
  -d '{"input":"如何优化深度学习"}' | jq '{learned_count: .learned_count, confidence: .confidence}'

echo ""
echo "========================================="
echo "  学习测试完成"
echo "========================================="
