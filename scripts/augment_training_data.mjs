import fs from 'fs';
const MIN_HUMAN = 5, MAX_HUMAN = 500, MIN_GPT = 1, MAX_GPT = 3000;
const KEYWORDS = ['解释','怎么','什么','如何','为什么','治疗','医学','代码','编程','学习','教育','量子','物理','化学','数学','历史','文化','艺术','心理','经济','法律'];
function hasDomain(s) { return KEYWORDS.some(k => s.includes(k)); }
const evalPath = process.argv[2] || 'data/eval_result.jsonl';
const sharegptPath = process.argv[3] || 'Old/sharegpt_pairs.zh.simplified_native.jsonl';
const trainPath = process.argv[4] || 'Old/data/cleaned_language_train.jsonl';
const maxAdd = parseInt(process.argv[5] || '5000', 10);
const existing = new Set();
const trainLines = (fs.readFileSync(trainPath, 'utf8') || '').split('\n').map(l => l.trim()).filter(Boolean);
for (const line of trainLines) {
  try {
    const v = JSON.parse(line);
    if (v.human) existing.add(String(v.human).trim());
  } catch (_) {}
}
console.error('existing train lines:', existing.size);
const added = [];
const sharegpt = (fs.readFileSync(sharegptPath, 'utf8') || '').split('\n').map(l => l.trim()).filter(Boolean);
for (const line of sharegpt) {
  if (added.length >= maxAdd) break;
  try {
    const raw = JSON.parse(line);
    const human = (raw.human || '').trim();
    const gpt = (raw.gpt || '').trim();
    if (human.length < MIN_HUMAN || human.length > MAX_HUMAN) continue;
    if (gpt.length < MIN_GPT || gpt.length > MAX_GPT) continue;
    if (existing.has(human)) continue;
    if (!hasDomain(human)) continue;
    existing.add(human);
    added.push(JSON.stringify({ human, gpt }));
  } catch (_) {}
}
if (added.length === 0) {
  console.error('no new samples to add');
  process.exit(0);
}
fs.appendFileSync(trainPath, added.join('\n') + '\n');
console.error('appended', added.length, 'lines to', trainPath);
