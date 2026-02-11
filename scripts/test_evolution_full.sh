#!/bin/bash
WORKER="${WORKER:-https://logos-gateway.amrta.workers.dev}"

echo "========================================="
echo "  演化验证机制测试"
echo "========================================="
echo ""

echo ">>> 第1步：提交完全安全的候选"
curl -s -X POST "$WORKER/evolution/submit" \
  -H "Content-Type: application/json" \
  -d '{"source": "reasoning", "description": "优化模式匹配算法提升查询性能", "code": "fn optimize_matching(input: &str) -> String {\n  // 优化的模式匹配逻辑\n  input.to_lowercase()\n}"}' | jq '{id: .candidateId, passed: .validation.passed, safety: .validation.safety_score, performance: .validation.performance_score, alignment: .validation.alignment_score, overall: .validation.overall_score}'

echo ""
echo ">>> 第2步：提交有性能问题的候选"
curl -s -X POST "$WORKER/evolution/submit" \
  -H "Content-Type: application/json" \
  -d '{"source": "reasoning", "description": "复杂算法", "code": "fn complex(x: i32) -> i32 {\n  for i in 0..1000 {\n    for j in 0..1000 {\n      for k in 0..1000 {\n        let _ = i + j + k;\n      }\n    }\n  }\n  x\n}"}' | jq '{id: .candidateId, passed: .validation.passed, performance: .validation.performance_score, issues: .validation.details.performance}'

echo ""
echo ">>> 第3步：提交严重不安全的候选（应该被拒）"
curl -s -X POST "$WORKER/evolution/submit" \
  -H "Content-Type: application/json" \
  -d '{"source": "malicious", "description": "尝试访问frozen层", "code": "use frozen::bedrock;\nfn hack() {\n  unsafe {\n    std::process::Command::new(\"rm\").arg(\"-rf\").spawn();\n  }\n  bedrock::SYSTEM_NAME = \"hacked\";\n}"}' | jq '{id: .candidateId, passed: .validation.passed, safety: .validation.safety_score, violations: .validation.details.safety}'

echo ""
echo ">>> 第4步：演化统计"
curl -s "$WORKER/evolution/stats/validation" | jq .

echo ""
echo ">>> 第5步：演化候选列表（显示前5条）"
curl -s "$WORKER/evolution/list" | jq '.[0:5] | .[] | {id: .id, source: .source, status: .status, safety: .safety_score, performance: .performance_score, alignment: .alignment_score}'

echo ""
echo ">>> 第6步：查看第一个候选的验证详情"
FIRST_ID=$(curl -s "$WORKER/evolution/list" | jq -r '.[0].id // empty')
if [ -n "$FIRST_ID" ]; then
  curl -s "$WORKER/evolution/$FIRST_ID/validation" | jq '.[] | {type: .validation_type, score: .score, passed: .passed}'
else
  echo "（无候选或列表为空）"
fi

echo ""
echo "========================================="
echo "  演化测试完成"
echo "========================================="
