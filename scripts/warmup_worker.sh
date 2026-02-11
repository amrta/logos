#!/bin/bash
WORKER="https://logos-gateway.amrta.workers.dev"
echo "Warming up Worker..."
curl -s "$WORKER/status" > /dev/null
curl -s -X POST "$WORKER/execute" -d '{"command":"status"}' -H "Content-Type: application/json" > /dev/null
curl -s "$WORKER/evolution/list" > /dev/null
echo "Worker warmed up"
