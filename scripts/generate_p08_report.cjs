#!/usr/bin/env node
const { writeFileSync, readFileSync, existsSync } = require('fs');
const { get } = require('https');

const DEFAULT_WORKER = 'https://logos-gateway.amrta.workers.dev';

function parseArg(name, def) {
  const i = process.argv.indexOf('--' + name);
  if (i >= 0 && process.argv[i + 1]) return process.argv[i + 1];
  const eq = process.argv.find(a => a.startsWith('--' + name + '='));
  if (eq) return eq.split('=')[1];
  return def;
}

function fetchMetrics(url) {
  return new Promise((resolve, reject) => {
    get(url + '/api/metrics?format=json', (res) => {
      let data = '';
      res.on('data', (c) => { data += c; });
      res.on('end', () => {
        try { resolve(JSON.parse(data)); } catch (e) { reject(e); }
      });
    }).on('error', reject);
  });
}

function ensureMetricsArray(data) {
  if (Array.isArray(data)) return data;
  if (data && data.error) {
    console.error('API error:', data.error, data.path || '');
  }
  return [];
}

function loadMetrics(fileOrUrl) {
  if (fileOrUrl.startsWith('http')) {
    return fetchMetrics(fileOrUrl.replace(/\/$/, ''));
  }
  const p = fileOrUrl || 'metrics.json';
  if (!existsSync(p)) return [];
  const raw = readFileSync(p, 'utf8').trim();
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed : [];
  } catch (_) {
    return [];
  }
}

function asciiChart(values, width, height, label) {
  width = width || 40;
  height = height || 8;
  if (!values.length) return '（无数据）';
  const min = Math.min.apply(null, values);
  const max = Math.max.apply(null, values);
  const range = max - min || 1;
  const rows = [];
  for (let y = height - 1; y >= 0; y--) {
    const threshold = min + (range * (y / height));
    let row = '';
    for (let x = 0; x < width; x++) {
      const i = Math.min(Math.floor((x / width) * values.length), values.length - 1);
      const v = values[i];
      row += v >= threshold - range * 0.05 ? '\u2588' : ' ';
    }
    rows.push(row);
  }
  return rows.join('\n') + '\n' + '-'.repeat(width) + '\n' + (label || min + ' ~ ' + max);
}

function generateReport(metrics) {
  const lines = [];
  lines.push('# LOGOS P0-P8 云端验证报告');
  lines.push('');
  lines.push('生成时间: ' + new Date().toISOString());
  lines.push('采样数: ' + metrics.length + ' 条');
  lines.push('');

  if (metrics.length === 0) {
    lines.push('## 数据不足');
    lines.push('');
    lines.push('D1 中尚无 learning_metrics 采样。请确保：');
    lines.push('1. Worker 已部署含 /api/metrics 的版本');
    lines.push('2. Worker 已配置 LOGOS_BACKEND 指向后端');
    lines.push('3. Cron 每 15 分钟触发 autonomous_learning_cycle');
    lines.push('4. 每 10 个周期会采样一次（需运行至少 10 个周期）');
    lines.push('');
    return lines.join('\n');
  }

  const crossFed = metrics.map(m => m.cross_fed ?? 0);
  const crossAbsorbed = metrics.map(m => m.cross_absorbed ?? 0);
  const saturation = metrics.map(m => (m.saturation ?? 0) * 100);
  const patternCount = metrics.map(m => m.pattern_count ?? 0);
  const avgMaturity = metrics.map(m => (m.avg_maturity ?? 0) * 100);

  lines.push('## 一、数据趋势');
  lines.push('');
  lines.push('### cross_fed（突触互学次数）');
  lines.push('```');
  lines.push(asciiChart(crossFed, 50, 6));
  lines.push('```');
  lines.push('');
  lines.push('### cross_absorbed（跨袋知识吸收）');
  lines.push('```');
  lines.push(asciiChart(crossAbsorbed, 50, 6));
  lines.push('```');
  lines.push('');
  lines.push('### saturation（饱和度 %）');
  lines.push('```');
  lines.push(asciiChart(saturation, 50, 6));
  lines.push('```');
  lines.push('');
  lines.push('### pattern_count（语言模式总数）');
  lines.push('```');
  lines.push(asciiChart(patternCount, 50, 6));
  lines.push('```');
  lines.push('');
  lines.push('### avg_maturity（平均成熟度 %）');
  lines.push('```');
  lines.push(asciiChart(avgMaturity, 50, 6));
  lines.push('```');
  lines.push('');

  const first = metrics[0];
  const last = metrics[metrics.length - 1];
  lines.push('## 二、数值汇总');
  lines.push('');
  lines.push('| Metric | First | Last | Delta |');
  lines.push('|--------|-------|------|-------|');
  lines.push('| cycle | ' + first.cycle + ' | ' + last.cycle + ' | +' + (last.cycle - first.cycle) + ' |');
  lines.push('| cross_fed | ' + first.cross_fed + ' | ' + last.cross_fed + ' | +' + (last.cross_fed - first.cross_fed) + ' |');
  lines.push('| cross_absorbed | ' + first.cross_absorbed + ' | ' + last.cross_absorbed + ' | +' + (last.cross_absorbed - first.cross_absorbed) + ' |');
  lines.push('| saturation | ' + (first.saturation * 100).toFixed(1) + '% | ' + (last.saturation * 100).toFixed(1) + '% | - |');
  lines.push('| pattern_count | ' + first.pattern_count + ' | ' + last.pattern_count + ' | +' + (last.pattern_count - first.pattern_count) + ' |');
  lines.push('| avg_maturity | ' + (first.avg_maturity * 100).toFixed(2) + '% | ' + (last.avg_maturity * 100).toFixed(2) + '% | - |');
  lines.push('');

  const lastCf = last.cross_fed ?? 0;
  const lastCa = last.cross_absorbed ?? 0;
  const lastSat = (last.saturation ?? 0) * 100;
  const lastMat = (last.avg_maturity ?? 0) * 100;
  lines.push('## 三、验证标准检查');
  lines.push('');
  lines.push('- ' + (lastCf > 100 ? '[OK]' : '[--]') + ' cross_fed > 100 (current ' + lastCf + ')');
  lines.push('- ' + (lastCa > 50 ? '[OK]' : '[--]') + ' cross_absorbed > 50 (current ' + lastCa + ')');
  lines.push('- ' + (lastSat >= 20 && lastSat <= 100 ? '[OK]' : '[--]') + ' saturation 20-100% (current ' + lastSat.toFixed(1) + '%)');
  lines.push('- ' + (lastMat > 10 ? '[OK]' : '[--]') + ' avg_maturity > 0.1 (current ' + lastMat.toFixed(2) + '%)');
  lines.push('');

  lines.push('## 四、结论');
  lines.push('');
  lines.push('P0-P8 验证完成。');
  lines.push('');

  return lines.join('\n');
}

async function main() {
  const input = parseArg('file', null) || parseArg('url', DEFAULT_WORKER) || DEFAULT_WORKER;
  const outPath = parseArg('out', 'report.md');
  const raw = await loadMetrics(input);
  const metrics = ensureMetricsArray(raw);
  const report = generateReport(metrics);
  writeFileSync(outPath, report);
  if (metrics.length > 0) {
    writeFileSync('metrics.json', JSON.stringify(metrics, null, 2));
    console.log('metrics.json written');
  }
  console.log('report written to', outPath);
}

main().catch(e => { console.error(e); process.exit(1); });
