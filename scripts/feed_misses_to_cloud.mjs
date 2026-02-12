#!/usr/bin/env node
/**
 * 把 test_cloud_1k 的 miss 问题配上 alpaca-zh 的 reference，POST 到云端 /feed/language-upload。
 * 用法: 先运行 test_cloud_1k.mjs 并把 misses 输出到文件，或直接从 alpaca 拉一批含 miss 的 instruction。
 * 本脚本：从 alpaca 拉 1000 条，调用 /execute 测一遍，收集 miss，再从 alpaca 取同 instruction 的 output 作为 gpt，批量 upload。
 */
const W = process.env.WORKER_URL || 'https://logos-gateway.amrta.workers.dev';
const TOTAL = 1000;
const BATCH = 100;

async function fetchBatch(offset) {
  const url = `https://datasets-server.huggingface.co/rows?dataset=shibing624/alpaca-zh&config=default&split=train&offset=${offset}&length=${Math.min(BATCH, 100)}`;
  const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS/1.0' } });
  if (!res.ok) return null;
  const data = await res.json();
  return data.rows || [];
}

async function testOne(q) {
  try {
    const r = await fetch(`${W}/execute`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: q, mode: 'natural' }),
    });
    const j = await r.json().catch(() => ({}));
    const reply = (j.reply || '').trim();
    if (reply.length < 10 || /Unknown command|没有关于|模式库里没有|暂时无法处理/.test(reply)) return null;
    return true;
  } catch (_) { return false; }
}

async function main() {
  const pairs = [];
  const targetMisses = parseInt(process.env.TARGET_MISSES, 10) || 50;
  for (let offset = 0; pairs.length < targetMisses; offset += BATCH) {
    const rows = await fetchBatch(offset);
    if (!rows || rows.length === 0) break;
    for (const item of rows) {
      const row = item.row || item;
      const human = (row.instruction || '').trim();
      const gpt = (row.output || '').trim();
      if (human.length < 5 || human.length > 500 || gpt.length < 4 || gpt.length > 3000) continue;
      const hit = await testOne(human);
      if (!hit) pairs.push({ human, gpt });
      if (pairs.length >= targetMisses) break;
    }
    if (offset > 50000) break;
  }
  if (pairs.length === 0) {
    console.log('No misses to feed');
    return;
  }
  const res = await fetch(`${W}/feed/language-upload`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pairs }),
  });
  const j = await res.json().catch(() => ({}));
  console.log('Fed', pairs.length, 'misses to cloud:', j);
}

main().catch(console.error);
