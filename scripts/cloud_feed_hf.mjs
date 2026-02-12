#!/usr/bin/env node
/**
 * 云端一次性投喂：从 HF 拉数据，分批 POST 到 Worker /feed/language-upload，写入 D1 language_pairs。
 * 用法: node scripts/cloud_feed_hf.mjs [dataset|preset] [human_field] [gpt_field]
 * 示例: node scripts/cloud_feed_hf.mjs alpaca-zh
 *       node scripts/cloud_feed_hf.mjs shibing624/alpaca-zh instruction output
 */
const WORKER = process.env.WORKER_URL || 'https://logos-gateway.amrta.workers.dev';
const BATCH = Math.min(parseInt(process.env.BATCH, 10) || 100, 100);
const MAX_TOTAL = parseInt(process.env.MAX_TOTAL, 10) || 5000;

const PRESETS = {
  'alpaca-zh': ['shibing624/alpaca-zh', 'instruction', 'output'],
  'belle': ['BelleGroup/train_0.5M_CN', 'instruction', 'output'],
  'sharegpt': ['shibing624/sharegpt_gpt4', 'human', 'assistant'],
  'firefly': ['YeungNLP/firefly-train-1.1M', 'input', 'target'],
  'medical': ['shibing624/medical', 'instruction', 'output'],
  'code': ['sahil2801/CodeAlpaca-20k', 'instruction', 'output'],
};

const [datasetArg, humanField, gptField] = process.argv.slice(2);
const preset = PRESETS[datasetArg];
const [dataset, hf, gf] = preset || [datasetArg, humanField || 'instruction', gptField || 'output'];
if (!dataset || !hf || !gf) {
  console.error('用法: node cloud_feed_hf.mjs <dataset|preset> [human_field] [gpt_field]');
  console.error('preset: alpaca-zh, belle, sharegpt, firefly, medical, code');
  process.exit(1);
}

const MIN_H = 5;
const MAX_H = 500;
const MAX_G = 3000;
const seen = new Set();

function toPairs(rows) {
  const pairs = [];
  for (const item of rows) {
    const row = item.row || item;
    let human = String(row[hf] || '').trim();
    let gpt = String(row[gf] || row.output || row.response || '').trim();
    if (human.length < MIN_H || human.length > MAX_H) continue;
    if (gpt.length < 4 || gpt.length > MAX_G) gpt = gpt.slice(0, MAX_G);
    const key = human.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    pairs.push({ human, gpt });
  }
  return pairs;
}

let offset = 0;
let totalStored = 0;

while (totalStored < MAX_TOTAL) {
  const url = `https://datasets-server.huggingface.co/rows?dataset=${encodeURIComponent(dataset)}&config=default&split=train&offset=${offset}&length=${BATCH}`;
  const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-CloudFeed/1.0' } });
  if (!res.ok) {
    const txt = (await res.text()).slice(0, 200);
    console.error('HF fetch failed:', res.status, txt);
    break;
  }
  const data = await res.json();
  const rows = data.rows || [];
  if (rows.length === 0) break;

  const pairs = toPairs(rows);
  if (pairs.length === 0) {
    offset += rows.length;
    if (rows.length < BATCH) break;
    continue;
  }

  const uploadRes = await fetch(`${WORKER}/feed/language-upload`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pairs }),
  });
  const json = await uploadRes.json().catch(() => ({}));
  const count = json.count || 0;
  totalStored += count;
  console.log(`offset ${offset} fetched ${rows.length} stored ${count} total ${totalStored}`);
  offset += rows.length;
  if (rows.length < BATCH) break;
  if (uploadRes.status !== 200) {
    console.error('upload error:', json);
    break;
  }
}

console.log('done, total stored in D1:', totalStored);
