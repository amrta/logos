import fs from 'fs';
import path from 'path';

const ROOT = process.cwd();
const OUT_DIR = path.join(ROOT, 'export');
const OUT_MD = path.join(OUT_DIR, 'LOGOS_CORE_EXPORT.md');

const FILES = [
  'Cargo.toml',
  'wrangler.toml',
  'README.md',
  '.cursorrules',
  'worker.js',
  'src/main.rs',
  'src/config.rs',
  'src/atom.rs',
  'src/orchestrator.rs',
  'src/language_pouch.rs',
  'src/pouch_trait.rs',
  'src/remote_pouch.rs',
  'src/resource_monitor.rs',
  'src/test_terminal.rs',
  'src/pouch_benchmark.rs',
  'src/pouch_capability_comparer.rs',
  'src/pouch_code_analyzer.rs',
  'src/pouch_defect_scanner.rs',
  'src/pouch_knowledge_retriever.rs',
  'src/pouch_pilot.rs',
  'src/pouch_programming.rs',
  'src/frozen/mod.rs',
  'src/frozen/bedrock.rs',
  'src/frozen/logic.rs',
  'src/bin/augment_training_data.rs',
  'src/bin/benchmark.rs',
  'src/bin/clean_language_data.rs',
  'src/bin/filter_zh_simplified.rs',
  'src/bin/stats.rs',
  'scripts/augment_training_data.mjs',
  'scripts/worker_deploy_and_check.sh',
  'scripts/language_train_and_eval.sh',
];

const DOCS = fs.readdirSync(path.join(ROOT, 'docs'))
  .filter(f => f.endsWith('.md'))
  .map(f => 'docs/' + f);

const ALL = [...FILES, ...DOCS];

function lang(pathname) {
  if (pathname.endsWith('.rs')) return 'rust';
  if (pathname.endsWith('.toml') || pathname.endsWith('.json')) return 'toml';
  if (pathname.endsWith('.js') || pathname.endsWith('.mjs')) return 'javascript';
  if (pathname.endsWith('.sh')) return 'shell';
  if (pathname.endsWith('.md')) return 'markdown';
  return 'text';
}

let buf = [];
buf.push('# LOGOS 核心导出 — 第三方评估/审计用');
buf.push('');
buf.push('**生成时间**: ' + new Date().toISOString());
buf.push('**用途**: 便于第三方评估或审计，单文件 Claude 兼容（按路径分块，可直接粘贴或上传）。');
buf.push('');
buf.push('---');
buf.push('');
buf.push('## 文件索引');
buf.push('');
buf.push('| 序号 | 路径 | 说明 |');
buf.push('|------|------|------|');

let idx = 0;
for (const rel of ALL) {
  idx++;
  const full = path.join(ROOT, rel);
  let note = '';
  if (rel.startsWith('src/bin')) note = 'CLI 工具';
  else if (rel === 'worker.js') note = 'CF Worker 入口';
  else if (rel.startsWith('docs/')) note = '文档';
  else if (rel === 'src/orchestrator.rs') note = '编排核心';
  else if (rel === 'src/language_pouch.rs') note = '语言模式核心';
  buf.push(`| ${idx} | \`${rel}\` | ${note} |`);
}

buf.push('');
buf.push('---');
buf.push('');
buf.push('## 文件内容');
buf.push('');

for (const rel of ALL) {
  const full = path.join(ROOT, rel);
  if (!fs.existsSync(full)) {
    buf.push('### ' + rel);
    buf.push('');
    buf.push('*(文件不存在或已排除)*');
    buf.push('');
    continue;
  }
  const stat = fs.statSync(full);
  if (!stat.isFile()) continue;
  let raw;
  try {
    raw = fs.readFileSync(full, 'utf8');
  } catch (_) {
    buf.push('### ' + rel);
    buf.push('');
    buf.push('*(无法读取，可能为二进制)*');
    buf.push('');
    continue;
  }
  const l = lang(rel);
  buf.push('### ' + rel);
  buf.push('');
  buf.push('```' + l);
  buf.push(raw.replace(/\r\n/g, '\n').trimEnd());
  buf.push('```');
  buf.push('');
}

fs.mkdirSync(OUT_DIR, { recursive: true });
fs.writeFileSync(OUT_MD, buf.join('\n'), 'utf8');
console.log('Written:', OUT_MD);
console.log('Size:', (fs.statSync(OUT_MD).size / 1024).toFixed(1), 'KB');
console.log('Files:', ALL.length);

const OUT_TAR = path.join(OUT_DIR, 'LOGOS_CORE_EXPORT.tar.gz');
const listFile = path.join(OUT_DIR, '.tar_list.txt');
const existing = ALL.filter(rel => fs.existsSync(path.join(ROOT, rel)));
if (existing.length > 0) {
  fs.writeFileSync(listFile, existing.join('\n'), 'utf8');
  const { execSync } = await import('child_process');
  execSync(`tar -czf "${OUT_TAR}" -C "${ROOT}" -T "${listFile}"`, { stdio: 'inherit' });
  fs.unlinkSync(listFile);
  console.log('Archive:', OUT_TAR, '(' + (fs.statSync(OUT_TAR).size / 1024).toFixed(1), 'KB)');
}
