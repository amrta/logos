#!/usr/bin/env bash
# 从 HuggingFace datasets 拉取并投喂到本地 backend。用法: ./feed_hf_to_backend.sh [dataset] [offset] [count]
# 示例: ./feed_hf_to_backend.sh shibing624/alpaca-zh 0 500
set -e
BACKEND="${BACKEND:-http://127.0.0.1:3000}"
DATASET="${1:-shibing624/alpaca-zh}"
OFFSET="${2:-0}"
COUNT="${3:-500}"
HF="instruction"
GF="output"
[[ "$DATASET" == *sharegpt* ]] && HF="human" && GF="assistant"
[[ "$DATASET" == *firefly* ]] && HF="input" && GF="target"

URL="https://datasets-server.huggingface.co/rows?dataset=${DATASET}&config=default&split=train&offset=${OFFSET}&length=${COUNT}"
TMP="$(dirname "$0")/../data/.hf_feed_tmp.json"
curl -s -H "User-Agent: LOGOS-Feed/1.0" "$URL" > "$TMP"
LINES=$(jq -r --arg h "$HF" --arg g "$GF" '
  .rows[]? | .row |
  {human: ((.[$h] // .instruction // .input // .human) | tostring | .[0:2000]),
   gpt:   ((.[$g] // .output // .assistant // .target) | tostring | .[0:5000])} |
  select(.human | length >= 4) | select(.gpt | length >= 4) | @json
' "$TMP")
rm -f "$TMP"
if [[ -z "$LINES" ]]; then
  echo "no rows"
  exit 0
fi
BODY=$(echo "$LINES" | tr '\n' '\n')
echo "$BODY" | head -1 | grep -q human || { echo "jq failed"; exit 1; }
RES=$(curl -s -X POST "$BACKEND/api/batch_teach" -H "Content-Type: text/plain; charset=utf-8" --data-binary @- <<< "$BODY")
echo "$RES" | jq .
