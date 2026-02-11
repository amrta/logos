import fs from 'fs';

const trainPath = 'Old/data/cleaned_language_train.jsonl';
const lines = fs.existsSync(trainPath)
  ? fs.readFileSync(trainPath, 'utf8').split('\n').filter(Boolean)
  : [];

const patterns = {};
lines.forEach(line => {
  let human = '';
  try {
    const o = JSON.parse(line);
    human = (o.human || o.input || '').trim();
  } catch (_) {}
  if (!human) return;
  if (/^(如何|怎么|怎样)/.test(human)) patterns['how'] = (patterns['how'] || 0) + 1;
  else if (/什么是|是什么|啥是/.test(human)) patterns['what'] = (patterns['what'] || 0) + 1;
  else if (/^为什么/.test(human)) patterns['why'] = (patterns['why'] || 0) + 1;
  else if (/[?？]$/.test(human)) patterns['question'] = (patterns['question'] || 0) + 1;
  else patterns['statement'] = (patterns['statement'] || 0) + 1;
});

const keywords = {};
const stopWords = new Set(['的', '是', '在', '了', '和', '与', '我', '你', '他', '她', '它']);
lines.forEach(line => {
  let human = '';
  try {
    human = (JSON.parse(line).human || '').trim();
  } catch (_) {}
  human.split(/[\s,，。！？;；:：]+/).filter(w => w.length > 1 && !stopWords.has(w)).forEach(word => {
    keywords[word] = (keywords[word] || 0) + 1;
  });
});

console.log('=== 问句类型分布 ===');
Object.entries(patterns).sort((a, b) => b[1] - a[1]).forEach(([type, count]) => {
  console.log(`${type}: ${count} (${lines.length ? ((count / lines.length) * 100).toFixed(1) : 0}%)`);
});
console.log('\n=== Top 20 关键词 ===');
Object.entries(keywords).sort((a, b) => b[1] - a[1]).slice(0, 20).forEach(([word, count]) => {
  console.log(`${word}: ${count}`);
});

const seeds = [
  { id: 'seed_how', pattern: '^(如何|怎么|怎样)', response_template: '步骤式回答', coverage: patterns['how'] || 0 },
  { id: 'seed_what', pattern: '(什么是|是什么|啥是)', response_template: '定义式回答', coverage: patterns['what'] || 0 },
  { id: 'seed_why', pattern: '^为什么', response_template: '因果式回答', coverage: patterns['why'] || 0 },
  { id: 'seed_question', pattern: '[?？]$', response_template: '疑问式回答', coverage: patterns['question'] || 0 },
  { id: 'seed_fallback', pattern: '.*', response_template: '通用回答', coverage: patterns['statement'] || 0 }
];
console.log('\n=== 种子规则 ===');
seeds.forEach(s => {
  console.log(`${s.id}: 覆盖${s.coverage}条 (${lines.length ? ((s.coverage / lines.length) * 100).toFixed(1) : 0}%)`);
});

const outDir = 'data';
if (!fs.existsSync(outDir)) fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(outDir + '/seed_rules.json', JSON.stringify(seeds, null, 2));
console.log('\n种子规则已保存到 data/seed_rules.json');
