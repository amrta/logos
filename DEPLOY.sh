#!/bin/bash
set -e

echo "=== LOGOS v5 部署脚本 ==="
echo ""

cd "$(dirname "$0")"
LOGOS_DIR="$(pwd)"

# Step 1: Apply fixes
echo "[1/6] 应用修复..."
if [ -d "DELIVERY" ]; then
  cp DELIVERY/worker.js ./worker.js
  cp DELIVERY/.github/workflows/cloud-language-train.yml ./.github/workflows/cloud-language-train.yml
  echo "  ✓ worker.js (handleTest URL 修复)"
  echo "  ✓ cloud-language-train.yml (CI action 名称修复)"
else
  echo "  ⚠ DELIVERY 目录不存在，跳过文件替换"
fi

# Step 2: Build
echo ""
echo "[2/6] 编译..."
cargo build --bin logos --bin clean_language_data 2>&1 | tail -3

# Step 3: Test
echo ""
echo "[3/6] 运行测试..."
TEST_OUTPUT=$(cargo test 2>&1)
PASSED=$(echo "$TEST_OUTPUT" | grep "test result:" | head -1)
echo "  $PASSED"

# Step 4: Clean start (fresh data dir)
echo ""
echo "[4/6] 初始化 data 目录..."
mkdir -p data
if [ ! -f "data/pouches.json" ]; then
  echo "[]" > data/pouches.json
  echo "  ✓ 创建空 pouches.json（仅语言尿袋）"
else
  echo "  ✓ pouches.json 已存在（保留已安装尿袋）"
fi

# Step 5: Language training (if source data exists)
echo ""
echo "[5/6] 语言训练..."
if [ -f "sharegpt_pairs.zh.simplified_native.jsonl" ]; then
  cargo run --bin clean_language_data -- sharegpt_pairs.zh.simplified_native.jsonl \
    data/cleaned_language.jsonl --split=0.1 2>&1 | tail -3
  LOGOS_DATA=./data ./target/debug/logos \
    --import data/cleaned_language_train.jsonl \
    --eval data/cleaned_language_test.jsonl 2>&1
  echo "  ✓ 训练完成"
else
  echo "  ⚠ 未找到 sharegpt_pairs.zh.simplified_native.jsonl，跳过训练"
  echo "  → 可稍后手动执行: ./scripts/language_train_and_eval.sh"
fi

# Step 6: Start
echo ""
echo "[6/6] 启动 LOGOS..."
echo ""
echo "选择启动方式:"
echo "  终端交互: cargo run --bin logos -- --terminal"
echo "  Web UI:   cargo run --bin logos"
echo "  然后访问: http://127.0.0.1:3000"
echo ""
echo "=== 部署就绪 ==="
