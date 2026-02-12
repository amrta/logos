#!/usr/bin/env bash
# 从 data/eval_result.jsonl 抽取 input，批量 POST 到 LOGOS 后端 /api/chat，触发 miss_buffer 写入。
# 学习周期会自动吸收这些 miss。需先启动 backend: cargo run
set -e
cd "$(dirname "$0")/.."
BACKEND="${BACKEND:-http://127.0.0.1:3000}"
SOURCE="${1:-data/eval_result.jsonl}"
LIMIT="${LIMIT:-100}"
DELAY="${DELAY:-1}"

if [ ! -f "$SOURCE" ]; then
  echo "文件不存在: $SOURCE"
  exit 1
fi

echo "来源: $SOURCE | 目标: $BACKEND | 限制: $LIMIT 条 | 间隔: ${DELAY}s"
count=0
while IFS= read -r line; do
  [ -z "$line" ] && continue
  [ $count -ge $LIMIT ] && break
  input=$(echo "$line" | jq -r . 2>/dev/null)
  [ -z "$input" ] || [ "$input" = "null" ] && continue
  esc=$(printf '%s' "$input" | jq -Rs .)
  curl -s -X POST "$BACKEND/api/chat" -H "Content-Type: application/json" -d "{\"message\":$esc}" > /dev/null
  count=$((count + 1))
  printf "\r已投喂 %d 条" $count
  sleep $DELAY
done < <(jq -c '.input' "$SOURCE" 2>/dev/null | head -n $LIMIT)

echo ""
echo "完成. 等待 2～3 个学习周期(约 3～5 分钟) 后，相同问题应可命中。"
