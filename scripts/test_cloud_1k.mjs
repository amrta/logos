#!/usr/bin/env node
/**
 * 测试云端语言能力：取 1000 个问题，逐个 POST /execute，统计命中率。
 * 用法: node scripts/test_cloud_1k.mjs
 */
const W = process.env.WORKER_URL || 'https://logos-gateway.amrta.workers.dev';
const TOTAL = 1000;
const BATCH = 100;

async function fetchQuestions() {
  const questions = [];
  for (let offset = 0; questions.length < TOTAL; offset += BATCH) {
    const url = `https://datasets-server.huggingface.co/rows?dataset=shibing624/alpaca-zh&config=default&split=train&offset=${offset}&length=${Math.min(BATCH, 100)}`;
    const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-Test/1.0' } });
    if (!res.ok) break;
    const data = await res.json();
    const rows = data.rows || [];
    for (const item of rows) {
      const q = (item.row?.instruction || '').trim();
      if (q.length >= 5 && q.length <= 500) questions.push(q);
      if (questions.length >= TOTAL) break;
    }
    if (rows.length < BATCH) break;
    if (offset > 50000) break;
  }
  return questions.slice(0, TOTAL);
}

function isHit(reply) {
  if (!reply || typeof reply !== 'string') return false;
  const r = reply.trim();
  if (r.length < 10) return false;
  if (/Unknown command|没有关于|模式库里没有|暂时无法处理/.test(r)) return false;
  return true;
}

async function testOne(q) {
  try {
    const res = await fetch(`${W}/execute`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ command: q, mode: 'natural' }),
    });
    const json = await res.json().catch(() => ({}));
    const reply = json.reply || json.error || '';
    return isHit(reply);
  } catch (_) {
    return false;
  }
}

async function main() {
  console.log('Fetching 1000 questions from alpaca-zh...');
  const questions = await fetchQuestions();
  console.log(`Got ${questions.length} questions`);

  const CONC = 10;
  let hits = 0;
  const start = Date.now();
  for (let i = 0; i < questions.length; i += CONC) {
    const batch = questions.slice(i, i + CONC);
    const results = await Promise.all(batch.map(testOne));
    hits += results.filter(Boolean).length;
    const done = Math.min(i + CONC, questions.length);
    if (done % 100 === 0 || done === questions.length) {
      const elapsed = ((Date.now() - start) / 1000).toFixed(1);
      console.log(`[${done}/${questions.length}] hits=${hits} rate=${(hits / done * 100).toFixed(1)}% elapsed=${elapsed}s`);
    }
  }
  const elapsed = ((Date.now() - start) / 1000).toFixed(1);
  console.log(`\nDone: ${hits}/${questions.length} hits, ${(hits / questions.length * 100).toFixed(1)}% rate, ${elapsed}s`);
}

main().catch(console.error);
