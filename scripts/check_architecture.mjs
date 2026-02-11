#!/usr/bin/env node
import { readFileSync, existsSync } from 'fs';
import { fileURLToPath } from 'url';
import { dirname, join } from 'path';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const checklistPath = join(root, 'docs', 'architecture_checklist.json');

const checklist = JSON.parse(readFileSync(checklistPath, 'utf8'));
const failures = [];
const shortboards = [];

function grepCheck(path, pattern) {
  const full = join(root, path);
  if (!existsSync(full)) return { ok: false, msg: `文件不存在: ${path}` };
  const content = readFileSync(full, 'utf8');
  const re = new RegExp(pattern);
  return { ok: re.test(content), msg: re.test(content) ? '命中' : `未命中: ${pattern}` };
}

for (const item of checklist.manager) {
  if (item.completion < 100) shortboards.push({ layer: '管理器', ...item });
  else if (item.check) {
    const r = grepCheck(item.check.path, item.check.pattern);
    if (!r.ok) failures.push({ layer: '管理器', name: item.name, msg: r.msg });
  }
}

for (const item of checklist.layer) {
  if (item.completion < 100) shortboards.push({ layer: '尿袋层', ...item });
  else if (item.check) {
    const r = grepCheck(item.check.path, item.check.pattern);
    if (!r.ok) failures.push({ layer: '尿袋层', name: item.name, msg: r.msg });
  }
}

console.log('=== 架构自动化检查 ===\n');
if (failures.length) {
  console.log('未通过（与 100% 契约不一致）:');
  failures.forEach(f => console.log('  [%s] %s: %s', f.layer, f.name, f.msg));
  console.log('');
}
console.log('短板（完成度 < 100%）:');
if (shortboards.length === 0) console.log('  无');
else shortboards.forEach(s => console.log('  [%s] %s %s%% - %s', s.layer, s.name, s.completion, s.detail));
console.log('');
console.log('汇总: 未通过 %d 项，短板 %d 项', failures.length, shortboards.length);
process.exit(failures.length > 0 ? 1 : 0);
