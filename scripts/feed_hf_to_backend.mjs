#!/usr/bin/env node
/**
 * 从 HuggingFace datasets 拉取 instruction 数据，直接 POST 到本地 backend /api/batch_teach。
 * 用法: node scripts/feed_hf_to_backend.mjs [dataset] [human_field] [gpt_field]
 * 示例: node scripts/feed_hf_to_backend.mjs shibing624/alpaca-zh instruction output
 *       node scripts/feed_hf_to_backend.mjs BelleGroup/train_0.5M_CN instruction output
 *       node scripts/feed_hf_to_backend.mjs shibing624/sharegpt_gpt4 human assistant
 */
import { readFileSync } from 'fs';

const BACKEND = process.env.BACKEND || 'http://127.0.0.1:3000';
const BATCH = parseInt(process.env.BATCH, 10) || 500;
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
  console.error('用法: node feed_hf_to_backend.mjs <dataset|preset> [human_field] [gpt_field]');
  console.error('preset: alpaca-zh, belle, sharegpt, firefly, medical, code');
  process.exit(1);
}

let offset = 0;
let totalTaught = 0;

while (totalTaught < MAX_TOTAL) {
  const url = `https://datasets-server.huggingface.co/rows?dataset=${encodeURIComponent(dataset)}&config=default&split=train&offset=${offset}&length=${BATCH}`;
  const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-Feed/1.0' } });
  if (!res.ok) {
    console.error('HF fetch failed:', res.status);
    break;
  }
  const data = await res.json();
  const rows = data.rows || [];
  if (rows.length === 0) break;

  const lines = [];
  for (const item of rows) {
    const row = item.row || item;
    const human = String(row[hf] || '').trim();
    const gpt = String(row[gf] || row.output || row.response || '').trim();
    if (human.length < 4 || gpt.length < 4) continue;
    lines.push(JSON.stringify({ human: human.slice(0, 2000), gpt: gpt.slice(0, 5000) }));
  }

  if (lines.length === 0) {
    offset += rows.length;
    if (rows.length < BATCH) break;
    continue;
  }

  const body = lines.join('\n');
  const teachRes = await fetch(`${BACKEND}/api/batch_teach`, {
    method: 'POST',
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
    body,
  });
  const json = await teachRes.json().catch(() => ({}));
  const taught = json.taught || 0;
  totalTaught += taught;
  console.log(`offset ${offset} fetched ${rows.length} taught ${taught} total ${totalTaught}`);
  offset += rows.length;
  if (rows.length < BATCH) break;
}

console.log('done, total taught:', totalTaught);
