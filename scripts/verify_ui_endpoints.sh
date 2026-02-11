#!/bin/sh
BASE="https://logos-gateway.amrta.workers.dev"
echo "=== /status ==="
curl -s "$BASE/status" | head -c 200
echo ""
echo ""
echo "=== /execute status ==="
curl -s -X POST "$BASE/execute" -H "Content-Type: application/json" -d '{"command":"status"}' | head -c 200
echo ""
echo ""
echo "=== /task/create ==="
curl -s -X POST "$BASE/task/create" -H "Content-Type: application/json" -d '{"description":"Phase 1收官"}' | head -c 300
