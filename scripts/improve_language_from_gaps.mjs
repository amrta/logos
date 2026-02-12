#!/usr/bin/env node
/**
 * 在随机问题里找不足，通过反哺机制让语言尿袋逼近大模型水平。
 * 从多源（eval_result、medical、sharegpt、stem、code 等未主投领域）随机抽样，
 * 测 Worker /execute，miss 则用 reference 反哺 → 自动写入 language_pairs。
 *
 * 用法: node scripts/improve_language_from_gaps.mjs [--rounds N] [--per-round M]
 * 示例: node scripts/improve_language_from_gaps.mjs --rounds 50 --per-round 20
 */
import { readFileSync, existsSync } from 'fs';

const W = process.env.WORKER_URL || 'https://logos-gateway.amrta.workers.dev';
const ROUNDS = parseInt(process.env.ROUNDS, 10) || 30;
const PER_ROUND = parseInt(process.env.PER_ROUND, 10) || 20;
const DELAY_MS = 500;

const HF_SOURCES = [
  { dataset: 'shibing624/medical', h: 'instruction', g: 'output' },
  { dataset: 'hfl/stem_zh_instruction', h: 'input', g: 'output' },
  { dataset: 'shibing624/sharegpt_gpt4', h: 'human', g: 'assistant' },
  { dataset: 'sahil2801/CodeAlpaca-20k', h: 'instruction', g: 'output' },
  { dataset: 'FinGPT/fingpt-sentiment-train', h: 'input', g: 'output' },
  { dataset: 'ShengbinYue/DISC-Law-SFT', h: 'input', g: 'output' },
  { dataset: 'YeungNLP/firefly-train-1.1M', h: 'input', g: 'target' },
  { dataset: 'BelleGroup/train_0.5M_CN', h: 'instruction', g: 'output' },
];

function isMiss(reply) {
  if (!reply || typeof reply !== 'string') return true;
  const r = reply.trim();
  if (r.length < 10) return true;
  if (/Unknown command|没有关于|模式库里没有|暂时无法处理|我不是大模型|超出当前能力/.test(r)) return true;
  return false;
}

async function testWorker(input) {
  try {
    const res = await fetch(`${W}/execute`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: input, mode: 'natural' }),
    });
    const j = await res.json().catch(() => ({}));
    return j.reply || '';
  } catch (_) {
    return '';
  }
}

async function feedToLanguagePairs(pairs) {
  if (pairs.length === 0) return 0;
  try {
    const res = await fetch(`${W}/feed/language-upload`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ pairs }),
    });
    const j = await res.json().catch(() => ({}));
    return res.ok ? (j.count || pairs.length) : 0;
  } catch (_) {
    return 0;
  }
}

function loadEvalPairs(path) {
  if (!existsSync(path)) return [];
  const lines = readFileSync(path, 'utf8').trim().split('\n');
  const pairs = [];
  for (const line of lines) {
    if (!line.trim()) continue;
    try {
      const o = JSON.parse(line);
      const input = (o.input || '').trim();
      const ref = (o.reference || '').trim();
      if (input.length >= 5 && input.length <= 500 && ref.length >= 4 && ref.length <= 3000) {
        pairs.push({ human: input, gpt: ref });
      }
    } catch (_) {}
  }
  return pairs;
}

async function fetchHfBatch(src, offset) {
  const len = Math.min(50, 100);
  const url = `https://datasets-server.huggingface.co/rows?dataset=${encodeURIComponent(src.dataset)}&config=default&split=train&offset=${offset}&length=${len}`;
  try {
    const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-Gap/1.0' } });
    if (!res.ok) return [];
    const data = await res.json();
    const rows = data.rows || [];
    const pairs = [];
    for (const item of rows) {
      const row = item.row || item;
      const human = String(row[src.h] || '').trim();
      const gpt = String(row[src.g] || row.output || '').trim();
      if (human.length >= 5 && human.length <= 500 && gpt.length >= 4 && gpt.length <= 3000) {
        pairs.push({ human, gpt });
      }
    }
    return pairs;
  } catch (_) {
    return [];
  }
}

function shuffle(arr) {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

async function main() {
  const args = process.argv.slice(2);
  let rounds = ROUNDS;
  let perRound = PER_ROUND;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--rounds' && args[i + 1]) rounds = parseInt(args[i + 1], 10);
    if (args[i] === '--per-round' && args[i + 1]) perRound = parseInt(args[i + 1], 10);
  }

  const evalPath = 'data/eval_result.jsonl';
  const evalPairs = loadEvalPairs(evalPath);
  console.log(`Eval 来源: ${evalPairs.length} 条`);

  const hfOffsets = {};
  for (const s of HF_SOURCES) hfOffsets[s.dataset] = Math.floor(Math.random() * 5000);

  let totalTested = 0;
  let totalMiss = 0;
  let totalFed = 0;

  for (let r = 0; r < rounds; r++) {
    const batch = [];
    if (evalPairs.length > 0) {
      for (let i = 0; i < Math.min(3, perRound); i++) {
        batch.push(evalPairs[Math.floor(Math.random() * evalPairs.length)]);
      }
    }
    const src = HF_SOURCES[r % HF_SOURCES.length];
    const offset = hfOffsets[src.dataset] + r * 50;
    const hfBatch = await fetchHfBatch(src, offset);
    if (hfBatch.length > 0) {
      const need = Math.max(0, perRound - batch.length);
      batch.push(...shuffle(hfBatch).slice(0, need));
    }
    hfOffsets[src.dataset] = offset + (hfBatch.length || 50);

    const toFeed = [];
    for (const { human, gpt } of batch) {
      const reply = await testWorker(human);
      totalTested++;
      if (isMiss(reply)) {
        totalMiss++;
        toFeed.push({ human, gpt });
      }
      await new Promise((x) => setTimeout(x, DELAY_MS));
    }
    if (toFeed.length > 0) {
      const n = await feedToLanguagePairs(toFeed);
      totalFed += n;
    }

    console.log(
      `[${r + 1}/${rounds}] tested=${totalTested} miss=${totalMiss} fed=${totalFed} rate=${totalTested ? (totalMiss / totalTested * 100).toFixed(1) : 0}%`
    );
  }

  console.log(`\n完成: 测试 ${totalTested} 条, 发现 miss ${totalMiss} 条, 反哺 ${totalFed} 条`);
}

main().catch(console.error);
