#!/usr/bin/env sh
ROOT="${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT"
BASE="${BASE:-http://127.0.0.1:3000}"
PASS=0
FAIL=0

check() {
  if [ "$1" = "0" ]; then PASS=$((PASS+1)); echo "  PASS: $2"; else FAIL=$((FAIL+1)); echo "  FAIL: $2"; fi
}

echo "=== 1. cargo clippy ==="
cargo clippy > /tmp/logos_verify_clippy.log 2>&1; CLIPPY_EXIT=$?
cat /tmp/logos_verify_clippy.log
if [ "$CLIPPY_EXIT" -eq 0 ]; then check 0 "clippy"; else check 1 "clippy"; fi

echo ""
echo "=== 2. cargo test ==="
cargo test > /tmp/logos_verify_test.log 2>&1; TEST_EXIT=$?
tail -25 /tmp/logos_verify_test.log
if [ "$TEST_EXIT" -eq 0 ] && grep -q "test result: ok.*0 failed" /tmp/logos_verify_test.log; then
  PASSED=$(grep "test result: ok" /tmp/logos_verify_test.log | grep "passed; 0 failed" | head -1 | sed -n 's/.* \([0-9]*\) passed.*/\1/p')
  check 0 "test ($PASSED passed)"
else
  check 1 "test"
fi

echo ""
echo "=== 3. 本地 E2E（需先起服务: LOGOS_DATA=./data ./target/debug/logos）==="
if ! curl -s -o /dev/null -w "%{http_code}" --connect-timeout 2 "$BASE/" 2>/dev/null | grep -q "200"; then
  echo "  SKIP: $BASE 未响应，请另起终端执行: cd $ROOT && LOGOS_DATA=./data ./target/debug/logos"
  echo "        再重新运行本脚本"
else
  R1=$(curl -s -X POST "$BASE/api/chat" -H "Content-Type: application/json" -d '{"message":"你好"}')
  if echo "$R1" | grep -q '"pouch":"language"' && echo "$R1" | grep -q '"status":"ok"'; then check 0 "E2E-1 你好"; else check 1 "E2E-1 你好"; fi

  R2=$(curl -s -X POST "$BASE/api/chat" -H "Content-Type: application/json" -d '{"message":"什么是量子纠缠"}')
  if echo "$R2" | grep -q '"status":"ok"'; then check 0 "E2E-2 量子纠缠(首次)"; else check 1 "E2E-2 量子纠缠(首次)"; fi

  R3=$(curl -s -X POST "$BASE/api/feedback" -H "Content-Type: application/json" -d '{"input":"什么是量子纠缠","signal":-1,"correction":"量子纠缠是两个粒子之间的非局域关联现象"}')
  if echo "$R3" | grep -q '"status":"ok"' && echo "$R3" | grep -q "applied"; then check 0 "E2E-3 纠正"; else check 1 "E2E-3 纠正"; fi

  R4=$(curl -s "$BASE/api/feedback_status")
  if echo "$R4" | grep -q '"absorbed"' && echo "$R4" | grep -q '"feedback_log"'; then check 0 "E2E-4 feedback_status"; else check 1 "E2E-4 feedback_status"; fi

  R5=$(curl -s -X POST "$BASE/api/chat" -H "Content-Type: application/json" -d '{"message":"什么是量子纠缠"}')
  if echo "$R5" | grep -q '"pouch":"language"' && echo "$R5" | grep -q "量子纠缠.*粒子"; then check 0 "E2E-5 纠正生效"; else check 1 "E2E-5 纠正生效"; fi
fi

echo ""
echo "=== 结果 ==="
echo "  PASS: $PASS   FAIL: $FAIL"
if [ "$FAIL" -eq 0 ]; then
  echo "  总体: 通过"
  exit 0
else
  echo "  总体: 未通过"
  exit 1
fi
