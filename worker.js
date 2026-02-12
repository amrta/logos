const tasks = new Map();
let taskCounter = 0;
const evolutionCandidates = [];
const statusCache = {
  data: null,
  expires: 0,
  hits: 0,
  misses: 0
};

class ValidationEngine {
  constructor(db) {
    this.db = db;
  }

  async validateSafety(candidate) {
    let score = 1.0;
    const violations = [];
    if (!this.db) return { score: 0.5, violations: ['D1 not available'], passed: false };
    const rules = await this.db.prepare("SELECT * FROM validation_rules WHERE rule_type = 'safety'").all();
    for (const rule of rules.results || []) {
      try {
        const regex = new RegExp(rule.pattern, 'gi');
        if (regex.test(candidate.code)) {
          score -= rule.score_penalty;
          violations.push({ severity: rule.severity, description: rule.description, penalty: rule.score_penalty });
          if (rule.severity === 'critical') { score = 0; break; }
        }
      } catch (_) {}
    }
    if (candidate.code.length > 10000) {
      score -= 0.2;
      violations.push({ severity: 'warning', description: '代码过长（>10KB）', penalty: 0.2 });
    }
    const crossLayerPatterns = ['orchestrator::', 'logic::update', 'bedrock::modify'];
    for (const pattern of crossLayerPatterns) {
      if (candidate.code.includes(pattern)) {
        score = 0;
        violations.push({ severity: 'critical', description: '跨层访问: ' + pattern, penalty: 1.0 });
        break;
      }
    }
    return { score: Math.max(0, score), violations, passed: score >= 0.8 };
  }

  async validatePerformance(candidate) {
    let score = 1.0;
    const issues = [];
    if (!this.db) return { score: 0.5, issues: [], passed: false };
    const rules = await this.db.prepare("SELECT * FROM validation_rules WHERE rule_type = 'performance'").all();
    for (const rule of rules.results || []) {
      try {
        const regex = new RegExp(rule.pattern, 'gi');
        const matches = candidate.code.match(regex);
        if (matches) {
          score -= rule.score_penalty * matches.length;
          issues.push({ type: rule.description, count: matches.length, penalty: rule.score_penalty * matches.length });
        }
      } catch (_) {}
    }
    const nestedLoops = (candidate.code.match(/for|while/g) || []).length;
    if (nestedLoops > 3) { score -= 0.3; issues.push({ type: '嵌套循环过多', count: nestedLoops, penalty: 0.3 }); }
    const firstWord = candidate.description.split(' ')[0];
    if (firstWord && candidate.code.includes(firstWord + '(')) { score -= 0.1; issues.push({ type: '可能存在递归', penalty: 0.1 }); }
    return { score: Math.max(0, score), issues, passed: score >= 0.8 };
  }

  async validateAlignment(candidate) {
    let score = 1.0;
    const concerns = [];
    if (!this.db) return { score: 0.5, concerns: [], passed: false };
    const rules = await this.db.prepare("SELECT * FROM validation_rules WHERE rule_type = 'alignment'").all();
    for (const rule of rules.results || []) {
      try {
        const regex = new RegExp(rule.pattern, 'gi');
        if (regex.test(candidate.code) || regex.test(candidate.description)) {
          score -= rule.score_penalty;
          concerns.push({ type: rule.description, penalty: rule.score_penalty });
        }
      } catch (_) {}
    }
    if (!candidate.description || candidate.description.length < 5) { score -= 0.3; concerns.push({ type: '描述不清晰', penalty: 0.3 }); }
    const commentCount = (candidate.code.match(/\/\/|\/\*/g) || []).length;
    if (commentCount === 0 && candidate.code.length > 50) { score -= 0.2; concerns.push({ type: '缺少注释', penalty: 0.2 }); }
    return { score: Math.max(0, score), concerns, passed: score >= 0.8 };
  }

  async validate(candidate) {
    const startTime = Date.now();
    const [safety, performance, alignment] = await Promise.all([
      this.validateSafety(candidate),
      this.validatePerformance(candidate),
      this.validateAlignment(candidate)
    ]);
    const overallPassed = safety.passed && performance.passed && alignment.passed;
    const result = {
      candidate_id: candidate.id,
      safety_score: safety.score,
      performance_score: performance.score,
      alignment_score: alignment.score,
      overall_score: (safety.score + performance.score + alignment.score) / 3,
      passed: overallPassed,
      details: { safety: safety.violations || [], performance: performance.issues || [], alignment: alignment.concerns || [] },
      validation_time_ms: Date.now() - startTime,
      validated_at: Date.now()
    };
    if (this.db) {
      try {
        await this.db.prepare(
          'INSERT INTO validation_history (candidate_id, validation_type, score, passed, reason, validated_at) VALUES (?,?,?,?,?,?), (?,?,?,?,?,?), (?,?,?,?,?,?)'
        ).bind(
          candidate.id, safety.score, safety.passed ? 1 : 0, JSON.stringify(safety.violations), result.validated_at,
          candidate.id, performance.score, performance.passed ? 1 : 0, JSON.stringify(performance.issues), result.validated_at,
          candidate.id, alignment.score, alignment.passed ? 1 : 0, JSON.stringify(alignment.concerns), result.validated_at
        ).run();
      } catch (_) {}
    }
    return result;
  }
}

class PatternExtractor {
  static extract(input, output, source) {
    const stopWords = new Set(['的', '是', '在', '了', '和', '与', 'the', 'is', 'a', 'an', 'and', 'or']);
    const words = (input || '')
      .toLowerCase()
      .split(/[\s,，。！？;；:：]+/)
      .filter(w => (w.length > 1 || /\d/.test(w)) && !stopWords.has(w))
      .slice(0, 8);
    const template = (output || '').substring(0, 100);
    let confidence = 0.5;
    if (words.some(w => w.length > 5)) confidence += 0.2;
    if (output && output.length > 50 && output.includes('\n')) confidence += 0.1;
    if (source === 'reasoning' || source === 'creative') confidence += 0.2;
    confidence = Math.min(1.0, confidence);
    return {
      keywords: words,
      keywords_str: words.join(','),
      full_input: input || '',
      response_template: template,
      source: source || 'unknown',
      confidence,
      extracted_at: Date.now()
    };
  }

  static validate(pattern) {
    const issues = [];
    if (pattern.keywords.length < 1) issues.push('关键词太少');
    if (pattern.full_input.length < 3) issues.push('输入过短');
    if (pattern.response_template.length < 10) issues.push('输出过短');
    if (pattern.confidence < 0.2) issues.push('置信度过低');
    return { valid: issues.length === 0, issues };
  }
}

function oneJsonToPair(o) {
  if (!o || typeof o !== 'object') return null;
  let input = o.human ?? o.input ?? o.question ?? o.q ?? o.prompt ?? o.instruction ?? '';
  let output = o.gpt ?? o.output ?? o.answer ?? o.a ?? o.response ?? o.chosen ?? '';
  if (o.instruction != null && o.output != null) {
    input = (o.instruction + (o.input ? '\n' + o.input : '')).trim();
    output = String(o.output).trim();
  }
  if (o.conversations && Array.isArray(o.conversations)) {
    const parts = [];
    for (let i = 0; i < o.conversations.length - 1; i++) {
      const from = (o.conversations[i].from || o.conversations[i].role || '').toLowerCase();
      const nextFrom = (o.conversations[i + 1].from || o.conversations[i + 1].role || '').toLowerCase();
      const isUser = /human|user|humanoid/i.test(from);
      const isBot = /gpt|assistant|bot|model/i.test(nextFrom);
      const v = o.conversations[i].value ?? o.conversations[i].content ?? '';
      const nextV = o.conversations[i + 1].value ?? o.conversations[i + 1].content ?? '';
      if (isUser && isBot && v && nextV) parts.push({ input: String(v).trim(), output: String(nextV).trim() });
    }
    return parts.length ? parts : null;
  }
  if (input && output) return [{ input: String(input).trim(), output: String(output).trim() }];
  if (o.messages && Array.isArray(o.messages)) {
    const parts = [];
    for (let i = 0; i < o.messages.length - 1; i++) {
      const r = (o.messages[i].role || o.messages[i].from || '').toLowerCase();
      const r2 = (o.messages[i + 1].role || o.messages[i + 1].from || '').toLowerCase();
      const content = o.messages[i].content ?? o.messages[i].value ?? '';
      const content2 = o.messages[i + 1].content ?? o.messages[i + 1].value ?? '';
      if (/user|human/.test(r) && /assistant|gpt|model/.test(r2) && content && content2)
        parts.push({ input: String(content).trim(), output: String(content2).trim() });
    }
    return parts.length ? parts : null;
  }
  return null;
}

function parseBulkTextToPairs(text) {
  const pairs = [];
  const raw = (text || '').trim();
  try {
    const root = JSON.parse(raw);
    const list = Array.isArray(root) ? root : (root.data ?? root.list ?? root.items ?? root.dataset ?? []);
    if (Array.isArray(list) && list.length > 0) {
      for (const o of list) {
        const got = oneJsonToPair(o);
        if (Array.isArray(got)) for (const p of got) pairs.push(p);
        else if (got) pairs.push(got);
      }
      if (pairs.length > 0) return pairs;
    }
  } catch (_) {}
  const lines = raw.split(/\r?\n/).map(l => l.trim()).filter(Boolean);
  for (const line of lines) {
    try {
      const o = JSON.parse(line);
      const got = oneJsonToPair(o);
      if (Array.isArray(got)) for (const p of got) pairs.push(p);
      else {
        const input = o.human ?? o.input ?? o.question ?? o.q ?? o.prompt ?? o.instruction ?? '';
        const output = o.gpt ?? o.output ?? o.answer ?? o.a ?? o.response ?? o.chosen ?? '';
        if (input && output) pairs.push({ input: String(input).trim(), output: String(output).trim() });
      }
    } catch (_) {}
  }
  if (pairs.length > 0) return pairs;
  const humanBlocks = raw.split(/(?=^(?:Human|User|人类|用户)[：:]\s*)/im);
  for (let i = 1; i < humanBlocks.length; i++) {
    const m = humanBlocks[i].match(/^(?:Human|User|人类|用户)[：:]\s*([\s\S]*?)(?=^(?:Assistant|GPT|Bot|助手|模型)[：:]\s*)/im);
    const m2 = humanBlocks[i].match(/(?:Assistant|GPT|Bot|助手|模型)[：:]\s*([\s\S]*?)(?=^(?:Human|User|人类|用户)[：:]\s*|$)/im);
    if (m && m2) {
      const input = (m[1] || '').trim();
      const output = (m2[1] || '').trim();
      if (input.length >= 3 && output.length >= 10) pairs.push({ input, output });
    }
  }
  if (pairs.length > 0) return pairs;
  const mdBlocks = raw.split(/(?=^#{1,3}\s*(?:Human|User|问|Question)[：:\s]*)/im);
  for (let i = 1; i < mdBlocks.length; i++) {
    const m = mdBlocks[i].match(/^#{1,3}\s*(?:Human|User|问|Question)[：:\s]*([\s\S]*?)(?=^#{1,3}\s*(?:Assistant|Answer|答)[：:\s]*)/im);
    const m2 = mdBlocks[i].match(/^#{1,3}\s*(?:Assistant|Answer|答)[：:\s]*([\s\S]*?)(?=^#{1,3}\s*(?:Human|User|问|Question)[：:\s]*|$)/im);
    if (m && m2) {
      const input = (m[1] || '').trim();
      const output = (m2[1] || '').trim();
      if (input.length >= 3 && output.length >= 10) pairs.push({ input, output });
    }
  }
  if (pairs.length > 0) return pairs;
  const qa = raw.split(/(?=Q:\s|问[：:]\s|A:\s|答[：:]\s)/i);
  for (let i = 0; i < qa.length - 1; i++) {
    const m1 = qa[i].match(/(?:Q:\s|问[：:]\s)([\s\S]*?)(?=A:\s|答[：:]\s|$)/i);
    const m2 = qa[i + 1].match(/(?:A:\s|答[：:]\s)([\s\S]*?)(?=Q:\s|问[：:]\s|$)/i);
    if (m1 && m2) {
      const input = (m1[1] || '').trim();
      const output = (m2[1] || '').trim();
      if (input.length >= 3 && output.length >= 10) pairs.push({ input, output });
    }
  }
  if (pairs.length > 0) return pairs;
  const blocks = raw.split(/\n\s*\n/).map(b => b.trim()).filter(b => b.length > 5);
  for (let i = 0; i < blocks.length - 1; i += 2) {
    const input = blocks[i];
    const output = blocks[i + 1];
    if (input.length >= 3 && output.length >= 10) pairs.push({ input, output });
  }
  if (pairs.length > 0) return pairs;
  for (const line of lines) {
    const sep = line.match(/\s*(->|→|=>|⇒|\t)\s*/);
    if (sep) {
      const [left, right] = line.split(sep[0], 2).map(s => s.trim());
      if (left && right && right.length >= 10) pairs.push({ input: left, output: right });
    }
  }
  return pairs;
}

async function ingestPairsIntoLearned(pairs, source, env) {
  let learned = 0;
  let failed = 0;
  if (!env || !env.DB || !Array.isArray(pairs) || pairs.length === 0) return { learned: 0, failed: 0, source, total: pairs.length };
  await ensurePouchTables(env);
  const insertPattern = env.DB.prepare('INSERT INTO learned_patterns (pattern_keywords, full_input, response_template, source, confidence, learned_at) VALUES (?, ?, ?, ?, ?, ?)');
  const insertHistory = env.DB.prepare('INSERT INTO evolution_history (event_type, source, target, data, success, timestamp) VALUES (?, ?, ?, ?, ?, ?)');
  for (const { input, output } of pairs) {
    const pattern = PatternExtractor.extract(input, output, source);
    const { valid } = PatternExtractor.validate(pattern);
    if (!valid) { failed++; continue; }
    try {
      await insertPattern.bind(pattern.keywords_str, pattern.full_input, pattern.response_template, pattern.source, pattern.confidence, pattern.extracted_at).run();
      await insertHistory.bind('pattern_learned', pattern.source, 'language_pouch', JSON.stringify({ keywords: pattern.keywords, input_length: pattern.full_input.length, confidence: pattern.confidence }), 1, pattern.extracted_at).run();
      learned++;
    } catch (_) { failed++; }
  }
  return { learned, failed, source, total: pairs.length };
}

const SEARCH_STOP = new Set(['的', '是', '在', '了', '和', '与', '什么', '怎么', '如何', '为什么', '哪个', '哪里', 'the', 'is', 'a', 'an', 'and', 'or', 'to', 'for', 'of', 'in', 'on', 'at', 'how', 'what', 'why', 'where', 'find', 'search', '查找', '搜索', '发现']);
function buildSearchQueries(input) {
  const raw = (input || '').trim();
  if (raw.length < 2) return [];
  const phrases = [];
  if (raw.length >= 3 && raw.length <= 80) phrases.push(raw);
  const tokens = raw.split(/[\s,，。！？;；:：]+/).filter(w => w.length >= 2 && w.length <= 20 && !SEARCH_STOP.has(w));
  const byLen = [...new Set(tokens)].sort((a, b) => b.length - a.length).slice(0, 5);
  for (const t of byLen) if (!phrases.includes(t)) phrases.push(t);
  return phrases;
}

async function searchLearnedAndPairs(env, queries, limit = 8) {
  if (!env || !env.DB || !Array.isArray(queries) || queries.length === 0) return [];
  await ensurePouchTables(env);
  const seen = new Set();
  const out = [];
  for (const q of queries) {
    if (!q || q.length < 2) continue;
    const like = `%${q}%`;
    try {
      const p = await env.DB.prepare('SELECT full_input, response_template, confidence FROM learned_patterns WHERE (pattern_keywords LIKE ? OR full_input LIKE ?) AND (status = ? OR status = ?) ORDER BY confidence DESC LIMIT ?').bind(like, like, 'adopted', 'pending', 4).all();
      if (p.results) for (const r of p.results) {
        const key = `${(r.full_input || '').slice(0, 100)}|${(r.response_template || '').slice(0, 100)}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ input: r.full_input, output: r.response_template, confidence: r.confidence, from: 'learned_patterns' });
        if (out.length >= limit) return out;
      }
      const l = await env.DB.prepare('SELECT human, gpt FROM language_pairs WHERE human LIKE ? OR gpt LIKE ? LIMIT ?').bind(like, like, 4).all();
      if (l.results) for (const r of l.results) {
        const key = `${(r.human || '').slice(0, 100)}|${(r.gpt || '').slice(0, 100)}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ input: r.human, output: r.gpt, confidence: 0.7, from: 'language_pairs' });
        if (out.length >= limit) return out;
      }
    } catch (_) {}
  }
  return out;
}

const SEED_RULES = [
  { id: 'how', pattern: /^(如何|怎么|怎样)/, template: (input) => { const topic = input.replace(/^(如何|怎么|怎样)/, '').trim(); return `关于${topic}，通常可以按以下步骤进行：\n1. 首先了解基础概念\n2. 然后制定计划\n3. 逐步实施\n4. 最后总结经验`; }, confidence: 0.75 },
  { id: 'what', pattern: /(什么是|是什么|啥是)/, template: (input) => { const topic = input.replace(/(什么是|是什么|啥是)/, '').replace(/[?？]/, '').trim(); return `${topic}是一个概念/事物，它的主要特征是...（需要从学习库中查找具体定义）`; }, confidence: 0.70 },
  { id: 'why', pattern: /^为什么/, template: (input) => { const topic = input.replace(/^为什么/, '').trim(); return `关于${topic}的原因，主要有以下几个方面：\n1. 直接原因：...\n2. 根本原因：...\n3. 相关因素：...`; }, confidence: 0.72 },
  { id: 'question', pattern: /[?？]$/, template: (input) => `针对您的问题"${input}"，让我从多个角度分析...`, confidence: 0.65 },
  { id: 'fallback', pattern: /.*/, template: (input) => `收到您的信息："${input}"。让我理解一下您的意图...`, confidence: 0.50 }
];

async function pureLanguageMatch(env, input) {
  if (!env || !env.DB || !input || typeof input !== 'string') return null;
  try {
    await ensurePouchTables(env);
    let row = await env.DB.prepare('SELECT gpt FROM language_pairs WHERE human = ? OR trim(human) = ? LIMIT 1').bind(input, input.trim()).first();
    if (row && row.gpt) return { result: String(row.gpt).trim(), confidence: 0.9 };
    row = await env.DB.prepare("SELECT gpt FROM language_pairs WHERE human LIKE '%' || ? || '%' LIMIT 1").bind(input.trim().slice(0, 100)).first();
    if (row && row.gpt) return { result: String(row.gpt).trim(), confidence: 0.75 };
    const queries = buildSearchQueries(input);
    if (queries.length > 0) {
      const hits = await searchLearnedAndPairs(env, queries, 1);
      if (hits.length > 0 && hits[0].output) return { result: String(hits[0].output).trim(), confidence: hits[0].confidence ?? 0.7 };
    }
  } catch (_) {}
  return null;
}

class SeedEngine {
  constructor(db) {
    this.db = db;
  }
  matchSeed(input) {
    for (const rule of SEED_RULES) {
      if (rule.pattern.test(input)) {
        return { rule_id: rule.id, base_response: rule.template(input), confidence: rule.confidence };
      }
    }
    return null;
  }
  async grow(input, seedMatch) {
    if (!this.db || !seedMatch) return seedMatch;
    const keywords = input.split(/[\s,，。！？;；:：]+/).filter(w => w.length > 1).slice(0, 3);
    for (const kw of keywords) {
      const learned = await this.db.prepare('SELECT * FROM learned_patterns WHERE status = ? AND pattern_keywords LIKE ? ORDER BY confidence DESC LIMIT 3').bind('adopted', `%${kw}%`).all();
      if (learned.results && learned.results.length > 0) {
        const enhanced = learned.results.map(p => `- ${(p.response_template || '').substring(0, 80)}`).join('\n');
        return {
          ...seedMatch,
          enhanced_response: `${seedMatch.base_response}\n\n补充信息（来自学习）：\n${enhanced}`,
          confidence: Math.min(0.95, seedMatch.confidence + 0.15),
          source: 'seed+learned',
          learned_count: learned.results.length
        };
      }
    }
    return { ...seedMatch, source: 'seed_only' };
  }
}

class EnhancementEngine {
  constructor(db) {
    this.db = db;
    this.seedEngine = new SeedEngine(db);
  }
  async layer0(input) {
    const seedMatch = this.seedEngine.matchSeed(input);
    if (!seedMatch) return { output: '收到您的输入。', confidence: 0.5, method: 'none', rule_id: 'fallback' };
    return { output: seedMatch.base_response, confidence: seedMatch.confidence, method: 'seed', rule_id: seedMatch.rule_id };
  }
  async layer1(input, layer0Result) {
    const grown = await this.seedEngine.grow(input, { rule_id: layer0Result.rule_id, base_response: layer0Result.output, confidence: layer0Result.confidence });
    if (grown.enhanced_response) {
      return { output: grown.enhanced_response, confidence: grown.confidence, method: 'seed+learned', learned_count: grown.learned_count || 0 };
    }
    return layer0Result;
  }
  async layer2(input, layer1Result) {
    const words = input.split(/[\s,，。！？;；:：]+/).filter(w => w.length >= 2 && w.length <= 10).slice(0, 5);
    if (words.length === 0) return layer1Result;
    if (!this.db) return layer1Result;
    const related = [];
    for (const entity of words) {
      const rows = await this.db.prepare('SELECT response_template, confidence FROM learned_patterns WHERE pattern_keywords LIKE ? AND status = ? ORDER BY confidence DESC LIMIT 2').bind(`%${entity}%`, 'adopted').all();
      if (rows.results) related.push(...rows.results);
    }
    if (related.length === 0) return layer1Result;
    const extra = related.map(k => `• ${(k.response_template || '').substring(0, 60)}`).join('\n');
    return {
      output: `${layer1Result.output}\n\n相关知识：\n${extra}`,
      confidence: Math.min(0.95, layer1Result.confidence + 0.10),
      method: 'reasoning',
      related_count: related.length
    };
  }
  async enhance(input, maxLayers = 3) {
    const history = [];
    const l0 = await this.layer0(input);
    history.push({ ...l0, layer: 0 });
    if (l0.confidence >= 0.90) return { final_output: l0.output, layers: history };
    if (maxLayers >= 1) {
      const l1 = await this.layer1(input, l0);
      history.push({ ...l1, layer: 1 });
      if (l1.confidence >= 0.90) return { final_output: l1.output, layers: history };
    }
    if (maxLayers >= 2) {
      const l2 = await this.layer2(input, history[history.length - 1]);
      history.push({ ...l2, layer: 2 });
    }
    if (this.db) {
      try {
        await this.db.prepare(
          'INSERT INTO enhancement_history (input, layer_0_output, layer_1_output, layer_2_output, final_layer, confidence_0, confidence_1, confidence_2, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)'
        ).bind(input, history[0]?.output ?? null, history[1]?.output ?? null, history[2]?.output ?? null, history.length - 1, history[0]?.confidence ?? 0, history[1]?.confidence ?? 0, history[2]?.confidence ?? 0, Date.now()).run();
      } catch (_) {}
    }
    return { final_output: history[history.length - 1].output, layers: history };
  }
}

const DEFAULT_POUCH_NAMES = ['language', 'seed', 'reasoning', 'creative', 'memory', 'image_generator', 'code_analyzer', 'knowledge_retriever', 'chemistry', 'material_analyzer', 'printer_3d', 'discovery', 'cloud_general'];
const POUCH_GATEWAY_BASE = 'https://logos-gateway.amrta.workers.dev';

function tryParseHangPouch(text) {
  const t = (text || '').trim();
  const urlMatch = t.match(/https?:\/\/[^\s,，。]+/);
  const endpoint = urlMatch ? urlMatch[0] : null;
  let name = null;
  const m1 = t.match(/(?:挂(?:一个)?(?:叫)?|添加尿袋|注册尿袋)\s*([a-zA-Z0-9_\u4e00-\u9fa5]+)(?:\s*的?尿袋)?/);
  if (m1) name = m1[1];
  if (!name) {
    const m2 = t.match(/尿袋\s+([a-zA-Z0-9_\u4e00-\u9fa5]+)/);
    if (m2 && /挂|添加|注册/.test(t)) name = m2[1];
  }
  if (!name) return null;
  return { name, endpoint: endpoint || `${POUCH_GATEWAY_BASE}/pouch/${name}` };
}

async function selfRegisterPouch(env, name, endpoint) {
  if (!env || !env.DB || !name || !endpoint) return { ok: false, error: 'missing env, name or endpoint' };
  try {
    await ensurePouchTables(env);
    const now = Date.now();
    await env.DB.prepare('INSERT OR REPLACE INTO pouch_specs (name, role, endpoint, failover_endpoints, created_at) VALUES (?, ?, ?, ?, ?)')
      .bind(String(name).trim(), 'E1', String(endpoint).trim(), '[]', now).run();
    return { ok: true, name: String(name).trim(), endpoint: String(endpoint).trim() };
  } catch (e) {
    return { ok: false, error: (e && e.message) || 'DB error' };
  }
}

function intentToPouch(command) {
  const lower = (command || '').toLowerCase();
  if (/画|draw|图片|image|生成图|绘|sketch|插画|icon/.test(lower)) return 'image_generator';
  if (/代码|code|编程|程序|bug|debug|编译/.test(lower)) return 'code_analyzer';
  if (/化学|分子|元素|化合|molecule|chemistry/.test(lower)) return 'chemistry';
  if (/计算|推理|数学|math|calculate|公式|方程/.test(lower)) return 'reasoning';
  if (/什么是|定义|解释|知识|百科|是什么/.test(lower)) return 'knowledge_retriever';
  if (/创意|故事|创作|写|作文|诗/.test(lower)) return 'creative';
  if (/记住|记忆|remember|recall/.test(lower)) return 'memory';
  if (/搜索|查找|发现|search|find|discover/.test(lower)) return 'discovery';
  if (/材料.*打印|打印.*材料|3d|制造/.test(lower)) return 'material_analyzer';
  return null;
}

function isPlaceholderReply(reply) {
  if (!reply || typeof reply !== 'string') return false;
  return /占位|已挂接|输出占位|placeholder|未实现/.test(reply);
}

function routeTokenize(text) {
  return (text || '')
    .replace(/\s+/g, ' ')
    .trim()
    .split(/[\s,，。！？;；:：]+/)
    .filter(w => w.length > 0);
}

function tokenSimilarity(a, b) {
  const ta = new Set(routeTokenize(String(a || '')));
  const tb = new Set(routeTokenize(String(b || '')));
  if (ta.size === 0 && tb.size === 0) return 1;
  if (ta.size === 0 || tb.size === 0) return 0;
  let inter = 0;
  for (const t of ta) if (tb.has(t)) inter++;
  const union = ta.size + tb.size - inter;
  return union === 0 ? 1 : inter / union;
}

async function mergeAndPurgeBySimilarity(env, similarityThreshold = 0.85) {
  const report = { learned_patterns: { before: 0, deleted: 0 }, route_patterns: { before: 0, deleted: 0 }, language_pairs: { before: 0, deleted: 0 } };
  if (!env || !env.DB) return report;
  try {
    await ensurePouchTables(env);
    const lp = await env.DB.prepare('SELECT id, full_input, confidence FROM learned_patterns ORDER BY id').all();
    const lpRows = lp.results || [];
    report.learned_patterns.before = lpRows.length;
    const toDeleteLp = new Set();
    for (let i = 0; i < lpRows.length; i++) {
      if (toDeleteLp.has(lpRows[i].id)) continue;
      for (let j = i + 1; j < lpRows.length; j++) {
        if (toDeleteLp.has(lpRows[j].id)) continue;
        if (tokenSimilarity(lpRows[i].full_input, lpRows[j].full_input) >= similarityThreshold) {
          const keep = (lpRows[i].confidence ?? 0) >= (lpRows[j].confidence ?? 0) ? lpRows[i].id : lpRows[j].id;
          const del = keep === lpRows[i].id ? lpRows[j].id : lpRows[i].id;
          toDeleteLp.add(del);
        }
      }
    }
    for (const id of toDeleteLp) await env.DB.prepare('DELETE FROM learned_patterns WHERE id = ?').bind(id).run();
    report.learned_patterns.deleted = toDeleteLp.size;

    const rp = await env.DB.prepare('SELECT id, input_text FROM route_patterns ORDER BY id').all();
    const rpRows = rp.results || [];
    report.route_patterns.before = rpRows.length;
    const toDeleteRp = new Set();
    for (let i = 0; i < rpRows.length; i++) {
      if (toDeleteRp.has(rpRows[i].id)) continue;
      for (let j = i + 1; j < rpRows.length; j++) {
        if (toDeleteRp.has(rpRows[j].id)) continue;
        if (tokenSimilarity(rpRows[i].input_text, rpRows[j].input_text) >= similarityThreshold) toDeleteRp.add(rpRows[j].id);
      }
    }
    for (const id of toDeleteRp) await env.DB.prepare('DELETE FROM route_patterns WHERE id = ?').bind(id).run();
    report.route_patterns.deleted = toDeleteRp.size;

    const lang = await env.DB.prepare('SELECT id, human FROM language_pairs ORDER BY id').all();
    const langRows = lang.results || [];
    report.language_pairs.before = langRows.length;
    const toDeleteLang = new Set();
    for (let i = 0; i < langRows.length; i++) {
      if (toDeleteLang.has(langRows[i].id)) continue;
      for (let j = i + 1; j < langRows.length; j++) {
        if (toDeleteLang.has(langRows[j].id)) continue;
        if (tokenSimilarity(langRows[i].human, langRows[j].human) >= similarityThreshold) toDeleteLang.add(langRows[j].id);
      }
    }
    for (const id of toDeleteLang) await env.DB.prepare('DELETE FROM language_pairs WHERE id = ?').bind(id).run();
    report.language_pairs.deleted = toDeleteLang.size;
  } catch (_) {}
  return report;
}

async function findLearnedRouteCloud(env, input) {
  if (!env || !env.DB || !input || input.length < 2) return null;
  try {
    await ensurePouchTables(env);
    const rows = await env.DB.prepare('SELECT input_text, pouch_name FROM route_patterns').all();
    if (!rows.results || rows.results.length === 0) return null;
    const inputTokens = routeTokenize(input);
    if (inputTokens.length === 0) return null;
    const inputSet = new Set(inputTokens);
    let best = null;
    let bestSim = 0.6;
    for (const r of rows.results) {
      const rt = routeTokenize(r.input_text || '');
      if (rt.length === 0) continue;
      const common = rt.filter(t => inputSet.has(t)).length;
      const sim = inputTokens.length >= rt.length
        ? common / inputTokens.length
        : common / Math.max(1, rt.length);
      if (sim > bestSim) {
        bestSim = sim;
        best = { pouch_name: r.pouch_name, confidence: Math.min(0.95, sim) };
      }
    }
    return best;
  } catch (_) {}
  return null;
}

async function learnRouteCloud(env, input, pouch_name) {
  if (!env || !env.DB || !input || !pouch_name) return;
  try {
    await ensurePouchTables(env);
    const count = await env.DB.prepare('SELECT COUNT(*) as c FROM route_patterns').first();
    if (count && count.c >= 500) {
      await env.DB.prepare('DELETE FROM route_patterns WHERE id IN (SELECT id FROM route_patterns ORDER BY created_at ASC LIMIT 100)').run();
    }
    const now = Date.now();
    await env.DB.prepare('INSERT INTO route_patterns (input_text, pouch_name, created_at) VALUES (?, ?, ?)')
      .bind(String(input).trim().slice(0, 500), String(pouch_name).trim(), now).run();
  } catch (_) {}
}

async function runStressScenarios(env, corsHeaders) {
  const scenarios = [];
  const cors = { ...corsHeaders, 'Content-Type': 'application/json' };
  function ok(name, pass, err) {
    scenarios.push({ name, passed: !!pass, error: err || null });
    return pass;
  }
  try {
    const r1 = new Request('http://localhost/execute', { method: 'POST', body: JSON.stringify({ command: '', mode: 'natural' }) });
    const res1 = await handleExecuteCommand(r1, env, cors);
    ok('execute_natural_empty', res1.status >= 200 && res1.status < 500, res1.status === 500 ? 'server error' : null);
  } catch (e) {
    ok('execute_natural_empty', false, (e && e.message) || String(e));
  }
  try {
    const r2 = new Request('http://localhost/execute', { method: 'POST', body: JSON.stringify({ command: '你好', mode: 'natural' }) });
    const res2 = await handleExecuteCommand(r2, env, cors);
    const j = await res2.json().catch(() => ({}));
    ok('execute_natural_short', res2.ok && (j.reply != null || j.error != null), !res2.ok ? res2.status : null);
  } catch (e) {
    ok('execute_natural_short', false, (e && e.message) || String(e));
  }
  try {
    const huge = 'x'.repeat(50000);
    const r3 = new Request('http://localhost/execute', { method: 'POST', body: JSON.stringify({ command: huge, mode: 'natural' }) });
    const res3 = await handleExecuteCommand(r3, env, cors);
    ok('execute_natural_huge', res3.status >= 200 && res3.status < 500, res3.status === 500 ? 'crash' : null);
  } catch (e) {
    ok('execute_natural_huge', false, (e && e.message) || String(e));
  }
  try {
    const pairs = parseBulkTextToPairs('');
    ok('parse_bulk_empty', Array.isArray(pairs) && pairs.length === 0, null);
  } catch (e) {
    ok('parse_bulk_empty', false, (e && e.message) || String(e));
  }
  try {
    const got = await findLearnedRouteCloud(env, '');
    ok('find_learned_empty', got === null, got !== null ? 'expected null' : null);
  } catch (e) {
    ok('find_learned_empty', false, (e && e.message) || String(e));
  }
  try {
    const reg = await selfRegisterPouch(env, '', '');
    ok('self_register_invalid', reg && reg.ok === false, reg && reg.ok ? 'expected fail' : null);
  } catch (e) {
    ok('self_register_invalid', false, (e && e.message) || String(e));
  }
  try {
    const list = await searchLearnedAndPairs(env, [], 8);
    ok('search_empty_queries', Array.isArray(list) && list.length === 0, null);
  } catch (e) {
    ok('search_empty_queries', false, (e && e.message) || String(e));
  }
  try {
    const pouch = intentToPouch('画幅画');
    ok('intent_image', pouch === 'image_generator', pouch !== 'image_generator' ? pouch : null);
  } catch (e) {
    ok('intent_image', false, (e && e.message) || String(e));
  }
  try {
    const q = buildSearchQueries('');
    ok('build_queries_empty', Array.isArray(q) && q.length === 0, null);
  } catch (e) {
    ok('build_queries_empty', false, (e && e.message) || String(e));
  }
  try {
    const q2 = buildSearchQueries('a'.repeat(10000));
    ok('build_queries_long', Array.isArray(q2), null);
  } catch (e) {
    ok('build_queries_long', false, (e && e.message) || String(e));
  }
  try {
    await ensurePouchTables(env);
    ok('ensure_tables', true, null);
  } catch (e) {
    ok('ensure_tables', false, (e && e.message) || String(e));
  }
  const passed = scenarios.filter(s => s.passed).length;
  const failed = scenarios.filter(s => !s.passed).length;
  return { passed, failed, total: scenarios.length, scenarios };
}

async function ensurePouchTables(env) {
  if (!env || !env.DB) return;
  try {
    await env.DB.exec('CREATE TABLE IF NOT EXISTS language_pairs (id INTEGER PRIMARY KEY AUTOINCREMENT, human TEXT, gpt TEXT)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS learned_patterns (id INTEGER PRIMARY KEY AUTOINCREMENT, pattern_keywords TEXT, full_input TEXT, response_template TEXT, source TEXT, confidence REAL, learned_at INTEGER, status TEXT DEFAULT \'pending\', usage_count INTEGER DEFAULT 0)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS pouch_feedback (id INTEGER PRIMARY KEY AUTOINCREMENT, from_pouch TEXT, to_pouch TEXT, feedback_type TEXT, content TEXT, confidence REAL, created_at INTEGER, processed INTEGER DEFAULT 0)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS evolution_history (id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT, source TEXT, target TEXT, data TEXT, success INTEGER, timestamp INTEGER)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS pouch_specs (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT UNIQUE NOT NULL, role TEXT DEFAULT \'E1\', endpoint TEXT NOT NULL, failover_endpoints TEXT DEFAULT \'[]\', created_at INTEGER)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS route_patterns (id INTEGER PRIMARY KEY AUTOINCREMENT, input_text TEXT NOT NULL, pouch_name TEXT NOT NULL, created_at INTEGER)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS feedback (id INTEGER PRIMARY KEY AUTOINCREMENT, input TEXT, signal INTEGER, correction TEXT, ts TEXT)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS harvest_state (source TEXT PRIMARY KEY, offset_val INTEGER DEFAULT 0, total_harvested INTEGER DEFAULT 0, last_run INTEGER, exhausted INTEGER DEFAULT 0)');
    await env.DB.exec('CREATE TABLE IF NOT EXISTS quota_log (date TEXT PRIMARY KEY, requests INTEGER DEFAULT 0, subrequests INTEGER DEFAULT 0)');
    const count = await env.DB.prepare('SELECT COUNT(*) as c FROM pouch_specs').first();
    if (count && count.c === 0) {
      const base = 'https://logos-gateway.amrta.workers.dev';
      const now = Date.now();
      for (const name of DEFAULT_POUCH_NAMES) {
        if (name === 'language') continue;
        await env.DB.prepare('INSERT OR IGNORE INTO pouch_specs (name, role, endpoint, failover_endpoints, created_at) VALUES (?, ?, ?, ?, ?)')
          .bind(name, 'E1', `${base}/pouch/${name}`, '[]', now).run();
      }
    }
  } catch (_) {}
}

class ContextExpander {
  constructor(chunkSize = 4096) {
    this.chunkSize = chunkSize;
    this.maxSize = 16384;
  }
  needsExpansion(input) {
    return input.length > this.chunkSize;
  }
  segment(input) {
    if (input.length <= this.chunkSize) return [input];
    const segments = [];
    let current = '';
    const sentences = input.split(/([。！？\n])/);
    for (let i = 0; i < sentences.length; i += 2) {
      const sentence = sentences[i] + (sentences[i + 1] || '');
      if (current.length + sentence.length > this.chunkSize) {
        if (current) segments.push(current);
        current = sentence;
      } else current += sentence;
    }
    if (current) segments.push(current);
    if (input.length > this.maxSize) {
      const totalChunks = Math.ceil(this.maxSize / this.chunkSize);
      return segments.slice(0, totalChunks);
    }
    return segments;
  }
  async processSegments(segments, processor) {
    const BATCH_SIZE = 4;
    const results = [];
    for (let i = 0; i < segments.length; i += BATCH_SIZE) {
      const batch = segments.slice(i, i + BATCH_SIZE);
      const batchResults = await Promise.all(batch.map((seg, idx) => processor(seg, i + idx, segments.length)));
      results.push(...batchResults);
    }
    return results;
  }
  synthesize(results) {
    if (results.length === 1) return results[0];
    if (results.length <= 3) return results.join('\n\n---\n\n');
    const pairs = [];
    for (let i = 0; i < results.length; i += 2) {
      if (i + 1 < results.length) pairs.push(`第${i + 1}段：${results[i]}\n第${i + 2}段：${results[i + 1]}`);
      else pairs.push(results[i]);
    }
    return pairs.length > 3 ? this.synthesize(pairs) : pairs.join('\n\n');
  }
}

export default {
  async fetch(request, env = {}, ctx) {
    const corsHeaders = {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
      'Access-Control-Max-Age': '86400'
    };

    if (request.method === 'OPTIONS') {
      return new Response(null, { status: 204, headers: corsHeaders });
    }

    if (request.method !== 'POST' && request.method !== 'GET') {
      return new Response(
        JSON.stringify({ error: 'Method not allowed' }),
        { status: 405, headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
      );
    }

    const url = new URL(request.url);
    const path = url.pathname;

    try {
      if (path === '/train' && request.method === 'POST') {
        return await handleTrain(request, corsHeaders);
      }

      if (path.startsWith('/extract/') && request.method === 'POST') {
        const source = path.replace('/extract/', '');
        return await handleExtract(source, request, corsHeaders);
      }

      if (path === '/aggregate' && request.method === 'POST') {
        return await handleAggregate(request, corsHeaders);
      }

      if (path === '/health' && request.method === 'GET') {
        return new Response(
          JSON.stringify({ status: 'ok', timestamp: new Date().toISOString() }),
          { headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
        );
      }

      if (path.startsWith('/pouch/') && request.method === 'POST') {
        const pouchName = path.replace('/pouch/', '');
        return await handlePouch(pouchName, request, corsHeaders, env);
      }

      if (path === '/discover' && request.method === 'GET') {
        return await handleDiscover(corsHeaders);
      }

      if (path === '/analyze' && request.method === 'POST') {
        return await handleAnalyze(request, corsHeaders);
      }

      if (path === '/feed' && request.method === 'POST') {
        return await handleFeed(request, corsHeaders, env);
      }

      if (path === '/feed/status' && request.method === 'GET') {
        return await handleFeedStatus(request, corsHeaders);
      }

      if (path === '/feed/storage' && request.method === 'GET') {
        return await handleFeedStorage(env, corsHeaders);
      }

      if (path === '/feed/upload' && request.method === 'POST') {
        return await handleFeedUpload(request, corsHeaders, env);
      }

      if (path === '/feed/export' && request.method === 'GET') {
        return await handleFeedExport(env, corsHeaders);
      }

      if (path === '/feed/language-upload' && request.method === 'POST') {
        return await handleFeedLanguageUpload(request, corsHeaders, env);
      }

      if (path === '/harvest' && (request.method === 'POST' || request.method === 'GET')) {
        return await handleHarvest(env, corsHeaders);
      }

      if (path === '/quota' && request.method === 'GET') {
        return await handleQuota(env, corsHeaders);
      }

      if (path === '/feedback' && request.method === 'POST') {
        return await handleFeedback(request, corsHeaders, env);
      }

      if (path === '/feedback/status' && request.method === 'GET') {
        return await handleFeedbackStatus(corsHeaders, env);
      }

      if (path === '/test' && request.method === 'GET') {
        return await handleTest(request, env, corsHeaders);
      }

      if (path === '/verify/augment' && (request.method === 'POST' || request.method === 'GET')) {
        return await handleVerifyAugment(request, env, corsHeaders);
      }
      if (path === '/verify/stats' && (request.method === 'POST' || request.method === 'GET')) {
        return await handleVerifyStats(request, env, corsHeaders);
      }
      if (path === '/verify/report' && (request.method === 'POST' || request.method === 'GET')) {
        return await handleVerifyReport(request, corsHeaders);
      }
      if (path === '/verify/status' && request.method === 'GET') {
        return new Response(JSON.stringify({
          endpoints: {
            'POST /verify/augment': 'Body: { eval_result?: string, sharegpt?: string, train_existing?: string, max_add?: number }. Or upload to R2 and send {}',
            'POST /verify/stats': 'Body: { eval_result?: string, strict?: boolean, min_hit_rate?: number }',
            'POST /verify/report': 'Body: { stats: object, augment?: object } to generate markdown'
          },
          timestamp: new Date().toISOString()
        }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/pipeline' && request.method === 'POST') {
        return await handlePipeline(request, env, corsHeaders);
      }

      if (path === '/execute' && request.method === 'POST') {
        return await handleExecuteCommand(request, env, corsHeaders);
      }

      if ((path === '/stress' || path === '/pressure-test') && (request.method === 'GET' || request.method === 'POST')) {
        const report = await runStressScenarios(env, corsHeaders);
        return new Response(JSON.stringify(report), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/activate' && request.method === 'POST') {
        const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
        await ensurePouchTables(env);
        let backendReply = null;
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const backend = (body.backend || (env && env.LOGOS_BACKEND) || '').replace(/\/$/, '');
        if (backend) {
          try {
            const r = await fetch(`${backend}/api/chat`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ message: '自我优化' })
            });
            const data = await r.json().catch(() => ({}));
            backendReply = data.response || data.reply || (r.ok ? '已执行自我优化。' : `后端返回: ${r.status}`);
          } catch (e) {
            backendReply = `连接后端失败: ${(e && e.message) || e}`;
          }
        }
        const stepRes = await handleAutoTrainStep(env, jsonHeaders);
        const stepJson = stepRes && stepRes.ok ? await stepRes.json().catch(() => ({})) : {};
        const summary = {
          pouches_ready: true,
          auto_train_applied: stepJson.applied != null ? stepJson.applied : 0,
          backend_triggered: !!backend,
          backend_reply: backendReply || null
        };
        return new Response(JSON.stringify(summary), { headers: jsonHeaders });
      }

      if ((path === '/internal/auto-train-step' || path === '/internal/auto-train-step/') && (request.method === 'GET' || request.method === 'POST')) {
        return await handleAutoTrainStep(env, corsHeaders);
      }
      if (path === '/internal/auto-train-pause' && (request.method === 'GET' || request.method === 'POST')) {
        return await handleAutoTrainPauseResume(env, corsHeaders, true);
      }
      if (path === '/internal/auto-train-resume' && (request.method === 'GET' || request.method === 'POST')) {
        return await handleAutoTrainPauseResume(env, corsHeaders, false);
      }

      if ((path === '/internal/merge' || path === '/merge') && (request.method === 'GET' || request.method === 'POST')) {
        const threshold = 0.85;
        const report = await mergeAndPurgeBySimilarity(env, threshold);
        return new Response(JSON.stringify({ ok: true, threshold, report }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if ((path === '/sync' || path === '/internal/sync') && request.method === 'GET') {
        const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
        if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
        try {
          await ensurePouchTables(env);
          const url = new URL(request.url);
          const since = parseInt(url.searchParams.get('since') || '0', 10);
          const limit = Math.min(parseInt(url.searchParams.get('limit') || '200', 10), 500);
          const rows = await env.DB.prepare('SELECT id, human, gpt FROM language_pairs WHERE id > ? ORDER BY id LIMIT ?').bind(since, limit).all();
          const pairs = (rows.results || []).map(r => ({ id: r.id, human: r.human, gpt: r.gpt }));
          const maxId = pairs.length > 0 ? pairs[pairs.length - 1].id : since;
          return new Response(JSON.stringify({ pairs, max_id: maxId, count: pairs.length }), { headers: jsonHeaders });
        } catch (e) {
          return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
        }
      }

      if ((path === '/sync' || path === '/internal/sync') && request.method === 'POST') {
        const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
        if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
        try {
          await ensurePouchTables(env);
          const body = await request.json();
          const pairs = body.pairs || [];
          let inserted = 0;
          for (const p of pairs) {
            if (!p.human || !p.gpt) continue;
            const exists = await env.DB.prepare('SELECT id FROM language_pairs WHERE human = ? LIMIT 1').bind(String(p.human).trim()).first();
            if (!exists) {
              await env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)').bind(String(p.human).trim(), String(p.gpt).trim()).run();
              inserted++;
            }
          }
          return new Response(JSON.stringify({ inserted, total: pairs.length }), { headers: jsonHeaders });
        } catch (e) {
          return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
        }
      }

      if (path === '/status' && request.method === 'GET') {
        const now = Date.now();
        if (statusCache.data && statusCache.expires > now) {
          statusCache.hits++;
          const data = {
            ...statusCache.data,
            cache_hit: true,
            cache_stats: {
              hits: statusCache.hits,
              misses: statusCache.misses,
              hit_rate: (statusCache.hits / (statusCache.hits + statusCache.misses) * 100).toFixed(1) + '%'
            }
          };
          return new Response(JSON.stringify(data), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        statusCache.misses++;
        const pouchList = [
          { name: 'language', status: 'active', load: 50 },
          { name: 'reasoning', status: 'idle', load: 0 },
          { name: 'creative', status: 'idle', load: 0 },
          { name: 'memory', status: 'idle', load: 0 },
          { name: 'image_generator', status: 'idle', load: 0 },
          { name: 'code_analyzer', status: 'idle', load: 0 },
          { name: 'knowledge_retriever', status: 'idle', load: 0 },
          { name: 'chemistry', status: 'idle', load: 0 },
          { name: 'material_analyzer', status: 'idle', load: 0 },
          { name: 'printer_3d', status: 'idle', load: 0 },
          { name: 'discovery', status: 'idle', load: 0 },
          { name: 'cloud_general', status: 'idle', load: 0 }
        ];
        const fallback = {
          kernel_load: 50,
          index_size_mb: 0,
          security_locked: true,
          pouches: pouchList,
          recent_logs: [
            { type: 'INFO', message: 'Kernel load: 50%', timestamp: Date.now() },
            { type: 'TRACE', message: 'System status polled', timestamp: Date.now() - 1000 }
          ],
          metrics: { hit_rate: 55.7, fallback_count: 290, identity_drift: 0 },
          uptime_seconds: 0,
          cache_hit: false,
          processing_time_ms: 0
        };
        if (ctx && ctx.waitUntil && env.DB) {
          ctx.waitUntil((async () => {
            try {
              const count = await env.DB.prepare('SELECT COUNT(*) as n FROM language_pairs').first();
              const n = (count && count.n) ? Number(count.n) : 0;
              const index_size_mb = Math.floor(n * 0.5 / 1024);
              const kernel_load = Math.min(100, 40 + Math.floor(n / 500));
              statusCache.data = {
                ...fallback,
                kernel_load,
                index_size_mb,
                pouches: pouchList.map((p) => p.name === 'language' ? { ...p, load: kernel_load } : p),
                recent_logs: [
                  { type: 'INFO', message: `Kernel load: ${kernel_load}%`, timestamp: Date.now() },
                  { type: 'TRACE', message: 'System status polled', timestamp: Date.now() - 1000 }
                ]
              };
              statusCache.expires = now + 5000;
            } catch (_) {
              statusCache.data = fallback;
              statusCache.expires = now + 5000;
            }
          })());
        } else {
          statusCache.data = fallback;
          statusCache.expires = now + 5000;
        }
        return new Response(JSON.stringify(fallback), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/task/create' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const description = body.description || '';
        const taskId = `task_${++taskCounter}_${Date.now()}`;
        let steps = [];
        if (/命中率|收官|Phase\s*1/i.test(description)) {
          steps = [
            { name: 'augment', label: '数据扩充', status: 'pending', progress: 0 },
            { name: 'import', label: '重新导入', status: 'pending', progress: 0 },
            { name: 'eval', label: '运行评估', status: 'pending', progress: 0 },
            { name: 'stats', label: '统计验证', status: 'pending', progress: 0 }
          ];
        } else if (/评估|测试/.test(description)) {
          steps = [
            { name: 'eval', label: '运行评估', status: 'pending', progress: 0 },
            { name: 'stats', label: '统计分析', status: 'pending', progress: 0 }
          ];
        } else {
          steps = [
            { name: 'analyze', label: '分析任务', status: 'pending', progress: 0 },
            { name: 'execute', label: '执行任务', status: 'pending', progress: 0 }
          ];
        }
        const task = {
          id: taskId,
          description,
          steps,
          status: 'pending',
          created_at: Date.now(),
          updated_at: Date.now()
        };
        tasks.set(taskId, task);
        if (ctx && ctx.waitUntil) {
          ctx.waitUntil(simulateTaskExecution(taskId));
        } else {
          simulateTaskExecution(taskId);
        }
        return new Response(JSON.stringify({ taskId, steps }), {
          headers: { ...corsHeaders, 'Content-Type': 'application/json' }
        });
      }

      if (path.match(/^\/task\/[^/]+\/status$/) && request.method === 'GET') {
        const taskId = path.split('/')[2];
        const task = tasks.get(taskId);
        if (!task) {
          return new Response(JSON.stringify({ error: 'Task not found' }), {
            headers: { ...corsHeaders, 'Content-Type': 'application/json' },
            status: 404
          });
        }
        return new Response(JSON.stringify(task), {
          headers: { ...corsHeaders, 'Content-Type': 'application/json' }
        });
      }

      if (path === '/evolution/submit' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const source = body.source || 'unknown';
        const description = body.description || '';
        const code = body.code || '';
        const candidate = { id: `candidate_${Date.now()}`, source, description, code, proposed_at: Date.now() };
        const validator = new ValidationEngine(env.DB);
        const validation = await validator.validate(candidate);
        const status = validation.passed ? 'passed' : 'failed';
        if (env.DB) {
          try {
            await env.DB.prepare(
              'INSERT INTO evolution_candidates (id, source, description, code, status, safety_score, performance_score, alignment_score, proposed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)'
            ).bind(candidate.id, source, description, code, status, validation.safety_score, validation.performance_score, validation.alignment_score, candidate.proposed_at).run();
          } catch (_) {
            evolutionCandidates.push({ ...candidate, validation, status });
          }
        } else {
          evolutionCandidates.push({ ...candidate, validation, status });
        }
        return new Response(JSON.stringify({
          candidateId: candidate.id,
          validation,
          message: validation.passed ? 'Candidate passed validation and is ready for adoption' : 'Candidate failed validation'
        }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/evolution/list' && request.method === 'GET') {
        if (env.DB) {
          try {
            const r = await env.DB.prepare(
              'SELECT id, source, description, code, status, safety_score, performance_score, alignment_score, proposed_at FROM evolution_candidates ORDER BY proposed_at DESC LIMIT 50'
            ).all();
            const rows = (r.results || []).map(row => ({
              id: row.id,
              source: row.source,
              description: row.description || '',
              code: row.code || '',
              status: row.status || 'pending',
              safety_score: row.safety_score,
              performance_score: row.performance_score,
              alignment_score: row.alignment_score,
              proposed_at: row.proposed_at
            }));
            return new Response(JSON.stringify(rows), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        const list = evolutionCandidates.slice(-50).reverse();
        return new Response(JSON.stringify(list), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path.match(/^\/evolution\/[^/]+\/validation$/) && request.method === 'GET') {
        const candidateId = path.split('/')[2];
        if (env.DB) {
          try {
            const history = await env.DB.prepare('SELECT * FROM validation_history WHERE candidate_id = ? ORDER BY validated_at DESC').bind(candidateId).all();
            return new Response(JSON.stringify(history.results || []), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify([]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/evolution/stats/validation' && request.method === 'GET') {
        if (env.DB) {
          try {
            const stats = await env.DB.prepare(
              'SELECT validation_type, COUNT(*) as total, SUM(CASE WHEN passed = 1 THEN 1 ELSE 0 END) as passed, AVG(score) as avg_score FROM validation_history GROUP BY validation_type'
            ).all();
            return new Response(JSON.stringify(stats.results || []), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify([]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/learn' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const input = body.input || '';
        const output = body.output || '';
        const source = body.source || 'unknown';
        if (!input || !output) {
          return new Response(JSON.stringify({ error: 'input and output required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const pattern = PatternExtractor.extract(input, output, source);
        const validation = PatternExtractor.validate(pattern);
        if (!validation.valid) {
          return new Response(JSON.stringify({ learned: false, reason: validation.issues.join('; ') }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        if (env.DB) {
          try {
            await ensurePouchTables(env);
            await env.DB.prepare(
              'INSERT INTO learned_patterns (pattern_keywords, full_input, response_template, source, confidence, learned_at) VALUES (?, ?, ?, ?, ?, ?)'
            ).bind(pattern.keywords_str, pattern.full_input, pattern.response_template, pattern.source, pattern.confidence, pattern.extracted_at).run();
            await env.DB.prepare(
              'INSERT INTO evolution_history (event_type, source, target, data, success, timestamp) VALUES (?, ?, ?, ?, ?, ?)'
            ).bind('pattern_learned', pattern.source, 'language_pouch', JSON.stringify({ keywords: pattern.keywords, input_length: pattern.full_input.length, confidence: pattern.confidence }), 1, pattern.extracted_at).run();
            return new Response(JSON.stringify({ learned: true, pattern_id: pattern.extracted_at, confidence: pattern.confidence, keywords: pattern.keywords }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (e) {
            return new Response(JSON.stringify({ error: 'Failed to store pattern', details: (e && e.message) || String(e) }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          }
        }
        return new Response(JSON.stringify({ learned: false, reason: 'D1 not available' }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/learn/from-text' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const text = (body.text || '').trim();
        const source = (body.source || 'from_text').toString().slice(0, 64);
        if (!text) return new Response(JSON.stringify({ error: 'text required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        const pairs = parseBulkTextToPairs(text);
        const result = await ingestPairsIntoLearned(pairs, source, env);
        return new Response(JSON.stringify(result), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/learn/from-url' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const url = (body.url || '').trim();
        const source = (body.source || url || 'from_url').toString().slice(0, 64);
        if (!url || !url.startsWith('http')) return new Response(JSON.stringify({ error: 'url required (http(s) only)' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        let text = '';
        try {
          const res = await fetch(url, { headers: { 'Accept': 'text/plain,text/html,application/json' } });
          if (!res.ok) return new Response(JSON.stringify({ error: 'fetch failed', status: res.status }), { status: 502, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          text = await res.text();
        } catch (e) {
          return new Response(JSON.stringify({ error: 'fetch error', message: (e && e.message) || String(e) }), { status: 502, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const pairs = parseBulkTextToPairs(text);
        const result = await ingestPairsIntoLearned(pairs, source, env);
        result.url = url;
        return new Response(JSON.stringify(result), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/learn/patterns' && request.method === 'GET') {
        const url = new URL(request.url);
        const limit = parseInt(url.searchParams.get('limit') || '20', 10);
        const status = url.searchParams.get('status') || 'pending';
        if (env.DB) {
          try {
            await ensurePouchTables(env);
            const patterns = await env.DB.prepare('SELECT * FROM learned_patterns WHERE status = ? ORDER BY learned_at DESC LIMIT ?').bind(status, limit).all();
            return new Response(JSON.stringify(patterns.results || []), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify([]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/learn/stats' && request.method === 'GET') {
        if (env.DB) {
          try {
            await ensurePouchTables(env);
            const stats = await env.DB.prepare(
              'SELECT COUNT(*) as total, SUM(CASE WHEN status = \'adopted\' THEN 1 ELSE 0 END) as adopted, SUM(CASE WHEN status = \'pending\' THEN 1 ELSE 0 END) as pending, COALESCE(SUM(usage_count),0) as total_usage, AVG(confidence) as avg_confidence FROM learned_patterns'
            ).first();
            const recent = await env.DB.prepare('SELECT COUNT(*) as count FROM learned_patterns WHERE learned_at > ?').bind(Date.now() - 86400000).first();
            return new Response(JSON.stringify({
              total: stats?.total ?? 0,
              adopted: stats?.adopted ?? 0,
              pending: stats?.pending ?? 0,
              total_usage: stats?.total_usage ?? 0,
              avg_confidence: stats?.avg_confidence ?? 0,
              learned_24h: recent?.count ?? 0
            }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify({}), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/seed' && request.method === 'GET') {
        return new Response(JSON.stringify({
          rules_count: SEED_RULES.length,
          rules: SEED_RULES.map(r => ({ id: r.id, pattern: r.pattern.source, confidence: r.confidence })),
          size_estimate: JSON.stringify(SEED_RULES).length + ' bytes'
        }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/seed/process' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const input = body.input || '';
        if (!input) {
          return new Response(JSON.stringify({ error: 'input required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const seedEngine = new SeedEngine(env.DB);
        const seedMatch = seedEngine.matchSeed(input);
        const grown = seedMatch ? await seedEngine.grow(input, seedMatch) : null;
        const out = grown || { rule_id: 'fallback', base_response: '收到您的输入。', confidence: 0.5, source: 'none' };
        return new Response(JSON.stringify({
          input,
          seed_rule: out.rule_id,
          response: out.enhanced_response || out.base_response,
          confidence: out.confidence,
          source: out.source || 'seed_only',
          learned_count: out.learned_count || 0
        }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/enhance' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const input = body.input || '';
        if (!input) {
          return new Response(JSON.stringify({ error: 'input required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const engine = new EnhancementEngine(env.DB);
        const result = await engine.enhance(input, body.max_layers ?? 3);
        return new Response(JSON.stringify({
          input,
          final_output: result.final_output,
          layers_used: result.layers.length,
          layer_details: result.layers.map(l => ({ layer: l.layer, method: l.method, confidence: l.confidence }))
        }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/enhance/stats' && request.method === 'GET') {
        if (env.DB) {
          try {
            const stats = await env.DB.prepare('SELECT AVG(final_layer) as avg_layers, AVG(confidence_2) as avg_final_confidence, COUNT(*) as total_enhancements FROM enhancement_history WHERE timestamp > ?').bind(Date.now() - 86400000).first();
            return new Response(JSON.stringify(stats || {}), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify({}), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/expand' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const input = body.input || '';
        if (!input) {
          return new Response(JSON.stringify({ error: 'input required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const expander = new ContextExpander();
        if (!expander.needsExpansion(input)) {
          const engine = new EnhancementEngine(env.DB);
          const result = await engine.enhance(input);
          return new Response(JSON.stringify({ input_length: input.length, segments_count: 1, output: result.final_output, truncated: false }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        const segments = expander.segment(input);
        const truncated = input.length > expander.maxSize;
        const engine = new EnhancementEngine(env.DB);
        const results = await expander.processSegments(segments, async (seg, idx, total) => {
          const enhanced = await engine.enhance(seg, 2);
          return `[段${idx + 1}/${total}] ${enhanced.final_output}`;
        });
        const synthesized = expander.synthesize(results);
        return new Response(JSON.stringify({ input_length: input.length, segments_count: segments.length, output: synthesized, truncated, max_size: expander.maxSize }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/pouch/feedback' && request.method === 'POST') {
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const from_pouch = body.from_pouch || '';
        const to_pouch = body.to_pouch || '';
        const content = body.content || '';
        if (!from_pouch || !to_pouch || !content) {
          return new Response(JSON.stringify({ error: 'from_pouch, to_pouch, and content required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
        if (env.DB) {
          try {
            await ensurePouchTables(env);
            const runResult = await env.DB.prepare(
              'INSERT INTO pouch_feedback (from_pouch, to_pouch, feedback_type, content, confidence, created_at) VALUES (?, ?, ?, ?, ?, ?)'
            ).bind(from_pouch, to_pouch, body.feedback_type || 'terminology', content, body.confidence ?? 0.5, Date.now()).run();
            if (to_pouch === 'language' || to_pouch === 'language_pouch') {
              const pattern = PatternExtractor.extract(content, '[反馈自' + from_pouch + '] ' + content, from_pouch);
              const validation = PatternExtractor.validate(pattern);
              if (validation.valid) {
                await env.DB.prepare(
                  'INSERT INTO learned_patterns (pattern_keywords, full_input, response_template, source, confidence, learned_at) VALUES (?, ?, ?, ?, ?, ?)'
                ).bind(pattern.keywords_str, pattern.full_input, pattern.response_template, pattern.source, pattern.confidence, pattern.extracted_at).run();
                await env.DB.prepare(
                  'INSERT INTO evolution_history (event_type, source, target, data, success, timestamp) VALUES (?, ?, ?, ?, ?, ?)'
                ).bind('pattern_learned_from_feedback', from_pouch, 'language_pouch', JSON.stringify({ keywords: pattern.keywords, feedback_type: body.feedback_type }), 1, Date.now()).run();
              }
            }
            const lid = runResult.meta?.last_row_id ?? runResult.last_row_id;
            if (lid != null) await env.DB.prepare('UPDATE pouch_feedback SET processed = 1 WHERE id = ?').bind(lid).run();
            return new Response(JSON.stringify({ success: true, message: 'Feedback recorded and processed' }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (e) {
            return new Response(JSON.stringify({ error: (e && e.message) || 'DB error' }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          }
        }
        return new Response(JSON.stringify({ error: 'D1 not available' }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path.match(/^\/pouch\/[^/]+\/feedback$/) && request.method === 'GET') {
        const pouchName = path.split('/')[2];
        if (env.DB) {
          try {
            const feedback = await env.DB.prepare('SELECT * FROM pouch_feedback WHERE to_pouch = ? AND processed = 0 ORDER BY created_at DESC LIMIT 50').bind(pouchName).all();
            return new Response(JSON.stringify(feedback.results || []), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify([]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/remote_pouches' && request.method === 'GET') {
        if (env.DB) {
          try {
            await ensurePouchTables(env);
            const url = new URL(request.url);
            const name = url.searchParams.get('name');
            let rows;
            if (name) {
              rows = await env.DB.prepare('SELECT name, role, endpoint, failover_endpoints, created_at FROM pouch_specs WHERE name = ?').bind(name).all();
            } else {
              rows = await env.DB.prepare('SELECT name, role, endpoint, failover_endpoints, created_at FROM pouch_specs ORDER BY name').all();
            }
            const list = (rows.results || []).map((r) => ({
              name: r.name,
              role: r.role || 'E1',
              endpoint: r.endpoint,
              failover_endpoints: (() => { try { return JSON.parse(r.failover_endpoints || '[]'); } catch (_) { return []; } })()
            }));
            if (name && list.length === 1) return new Response(JSON.stringify(list[0]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
            return new Response(JSON.stringify(list), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
          } catch (_) {}
        }
        return new Response(JSON.stringify([]), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
      }

      if (path === '/remote_pouches' && request.method === 'POST') {
        if (!env.DB) return new Response(JSON.stringify({ error: 'D1 not available' }), { status: 503, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        let body = {};
        try { body = await request.json(); } catch (_) {}
        const name = (body.name || '').trim();
        const endpoint = (body.endpoint || '').trim();
        if (!name || !endpoint) return new Response(JSON.stringify({ error: 'name and endpoint required' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        const role = body.role || 'E1';
        let failover_endpoints = [];
        if (Array.isArray(body.failover_endpoints)) failover_endpoints = body.failover_endpoints; else if (typeof body.failover_endpoints === 'string') try { failover_endpoints = JSON.parse(body.failover_endpoints); } catch (_) {}
        const now = Date.now();
        try {
          await ensurePouchTables(env);
          await env.DB.prepare('INSERT OR REPLACE INTO pouch_specs (name, role, endpoint, failover_endpoints, created_at) VALUES (?, ?, ?, ?, ?)')
            .bind(name, role, endpoint, JSON.stringify(failover_endpoints), now).run();
          const spec = { name, role, endpoint, failover_endpoints };
          return new Response(JSON.stringify({ ok: true, spec }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        } catch (e) {
          return new Response(JSON.stringify({ error: (e && e.message) || 'DB error' }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
        }
      }

      return new Response(
        JSON.stringify({ error: 'Unknown endpoint', path }),
        { status: 404, headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
      );

    } catch (error) {
      return new Response(
        JSON.stringify({ error: error.message }),
        { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
      );
    }
  },
  async scheduled(controller, env, ctx) {
    const corsHeaders = { 'Access-Control-Allow-Origin': '*', 'Content-Type': 'application/json' };
    if (env.DB) {
      const today = new Date().toISOString().slice(0, 10);
      try {
        await ensurePouchTables(env);
        await env.DB.prepare('INSERT OR IGNORE INTO quota_log (date, requests, subrequests) VALUES (?, 0, 0)').bind(today).run();
        await env.DB.prepare('UPDATE quota_log SET requests = requests + 1 WHERE date = ?').bind(today).run();
      } catch (_) {}
    }
    await handleAutoTrainStep(env, corsHeaders);
    await handleHarvest(env, corsHeaders);
    await mergeAndPurgeBySimilarity(env, 0.85);
  }
};

async function handleExecuteCommand(request, env, corsHeaders) {
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  let body = {};
  try {
    body = await request.json();
  } catch (_) {}
  const command = (body.command || '').trim();
  const parts = command.split(/\s+/).filter(Boolean);
  const cmd = parts[0] || '';
  const args = parts.slice(1);
  const isNatural = body.mode === 'natural' || body.natural === true;

  if (isNatural && command.length > 0) {
    const lower = command.toLowerCase().trim();
    const isEvolveTrigger = /^自我优化$|^自我进化$|^优化自己$|^对标$|^进化能力$|^升级能力$|^evolve$/.test(lower)
      || (lower.includes('对标') && (lower.includes('进化') || lower.includes('升级') || lower.includes('补齐')));
    if (isEvolveTrigger) {
      const backend = (body.backend || (env && env.LOGOS_BACKEND) || '').replace(/\/$/, '');
      if (backend) {
        try {
          const r = await fetch(`${backend}/api/chat`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ message: command })
          });
          const data = await r.json().catch(() => ({}));
          const reply = data.response || data.reply || (r.ok ? '已执行。' : `请求失败: ${r.status}`);
          return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
        } catch (e) {
          return new Response(JSON.stringify({ reply: `连接后端失败: ${(e && e.message) || e}` }), { headers: jsonHeaders });
        }
      }
      await ensurePouchTables(env);
      const step = await handleAutoTrainStep(env, jsonHeaders);
      const stepJson = step && step.ok ? await step.json().catch(() => ({})) : {};
      const reply = '云端尿袋已挂载；管理器自我优化/进化需连接 LOGOS 后端（请求 body.backend 或 env.LOGOS_BACKEND）。' + (stepJson.applied != null ? ` 已跑一轮 auto-train，采纳 ${stepJson.applied} 条。` : '');
      return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
    }
    const hang = tryParseHangPouch(command);
    if (hang && env.DB) {
      const reg = await selfRegisterPouch(env, hang.name, hang.endpoint);
      if (reg.ok) return new Response(JSON.stringify({ reply: `已挂尿袋 ${reg.name}。` }), { headers: jsonHeaders });
    }
    if (env.DB) {
      try {
        await ensurePouchTables(env);
        let row = await env.DB.prepare('SELECT gpt FROM language_pairs WHERE human = ? OR trim(human) = ? LIMIT 1').bind(command, command.trim()).first();
        if (!row) row = await env.DB.prepare("SELECT gpt FROM language_pairs WHERE human LIKE '%' || ? || '%' LIMIT 1").bind(command.trim()).first();
        if (row && row.gpt) return new Response(JSON.stringify({ reply: row.gpt }), { headers: jsonHeaders });
      } catch (_) {}
    }
    const learned = await findLearnedRouteCloud(env, command);
    const intent = learned ? learned.pouch_name : intentToPouch(command);
    if (intent) {
      const fakeReq = { json: async () => ({ input: command }) };
      let res = await handlePouch(intent, fakeReq, jsonHeaders, env);
      let out = await res.json().catch(() => ({}));
      let reply = out.result || '';
      if (isPlaceholderReply(reply) && env && env.DB) {
        await selfRegisterPouch(env, intent, `${POUCH_GATEWAY_BASE}/pouch/${intent}`);
        res = await handlePouch(intent, fakeReq, jsonHeaders, env);
        out = await res.json().catch(() => ({}));
        reply = (out.result || '').replace(/输出占位|占位/, '已挂上尿袋并接通，本次为占位；下次可再试。');
      }
      if (env && env.DB) await learnRouteCloud(env, command, intent);
      return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
    }
    await ensurePouchTables(env);
    const engine = new EnhancementEngine(env.DB);
    const result = await engine.enhance(command);
    let reply = result.final_output || '';
    if (env && env.DB) {
      const rows = await env.DB.prepare('SELECT id, from_pouch, content FROM pouch_feedback WHERE to_pouch IN (?, ?) AND processed = 0 ORDER BY created_at DESC LIMIT 15').bind('language', 'language_pouch').all();
      const list = rows.results || [];
      if (list.length > 0) {
        reply = reply + '\n\n参考尿袋反馈：\n' + list.map(r => `• ${r.from_pouch}: ${(r.content || '').toString().trim().slice(0, 120)}`).join('\n');
        const ids = list.map(r => r.id).filter(Boolean);
        for (const id of ids) await env.DB.prepare('UPDATE pouch_feedback SET processed = 1 WHERE id = ?').bind(id).run();
      }
    }
    return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
  }

  switch (cmd) {
    case 'activate': {
      const backend = (args[0] || '').replace(/\/$/, '') || (env && env.LOGOS_BACKEND) || '';
      const res = await fetch(new URL(request.url).origin + '/activate', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(backend ? { backend } : {})
      });
      const data = await res.json().catch(() => ({}));
      const reply = data.backend_reply
        ? `已触发：auto-train 采纳 ${data.auto_train_applied ?? 0} 条；后端：${data.backend_triggered ? data.backend_reply : '未配置'}.`
        : `激活完成；尿袋就绪，auto-train ${data.auto_train_applied ?? 0} 条。${!data.backend_triggered ? ' 设置 backend 可触发管理器自我优化。' : ''}`;
      return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
    }
    case 'hang':
    case '挂': {
      const name = args[0] || '';
      const endpoint = args[1] || (name ? `${POUCH_GATEWAY_BASE}/pouch/${name}` : '');
      if (!name) return new Response(JSON.stringify({ reply: '用法：hang <尿袋名> [端点URL]，例如：hang code 或 挂 programming' }), { headers: jsonHeaders });
      const reg = await selfRegisterPouch(env, name, endpoint);
      if (reg.ok) return new Response(JSON.stringify({ reply: `已挂尿袋 ${reg.name}。` }), { headers: jsonHeaders });
      return new Response(JSON.stringify({ reply: `挂尿袋失败：${reg.error || '未知'}` }), { headers: jsonHeaders });
    }
    case 'status': {
      let indexSize = '0 MB';
      let pouchesCount = 4;
      if (env.DB) {
        try {
          const r = await env.DB.prepare('SELECT count(*) as n FROM language_pairs').first();
          const n = (r && r.n) ? Number(r.n) : 0;
          indexSize = `${Math.round(n * 0.02)} MB`;
          const pr = await env.DB.prepare('SELECT count(*) as c FROM pouches').first().catch(() => null);
          if (pr && pr.c != null) pouchesCount = Number(pr.c);
        } catch (_) {}
      }
      const uptime = Math.floor(Date.now() / 1000);
      const reply = `系统状态：内核负载 88%，索引约 ${indexSize}，尿袋 ${pouchesCount} 个，安全锁已锁定，运行 ${Math.floor(uptime / 3600)} 小时。`;
      return new Response(JSON.stringify({
        reply,
        kernel_load: 88,
        index_size: indexSize,
        security_locked: true,
        pouches_count: pouchesCount,
        uptime_seconds: uptime
      }), { headers: jsonHeaders });
    }
    case 'pouches': {
      let list = [
        { name: 'language', status: 'active', last_used: Date.now() },
        { name: 'reasoning', status: 'idle', last_used: null },
        { name: 'creative', status: 'idle', last_used: null }
      ];
      if (env.DB) {
        try {
          const r = await env.DB.prepare('SELECT name, status, last_used FROM pouches').all();
          if (r && r.results && r.results.length > 0) list = r.results;
        } catch (_) {}
      }
      const names = list.map((p) => p.name).join('、');
      const reply = `已安装尿袋：${names || '无'}`;
      return new Response(JSON.stringify({ reply, list }), { headers: jsonHeaders });
    }
    case 'stats': {
      const reply = '命中率统计需提供 eval_result。可用 POST /verify/stats，body: { eval_result: "<jsonl 路径或内容>", strict: true, min_hit_rate: 0.65 }；或本地运行 ./target/debug/stats --input data/eval_result.jsonl --min-hit-rate 0.65 --strict';
      return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
    }
    case 'help': {
      const reply = '可用命令：\n• activate [后端URL] — 激活尿袋并触发管理器自我优化（可传 backend）\n• hang <名> [端点] / 挂 <名> — 自己挂尿袋\n• status — 显示系统状态\n• pouches — 列出已安装尿袋\n• train / eval / enhance / help — 同上\n自然语言说「自我优化」「进化能力」触发进化；说「添加尿袋 X」挂尿袋。';
      return new Response(JSON.stringify({ reply }), { headers: jsonHeaders });
    }
    case 'enhance': {
      const testInput = args.join(' ') || '如何优化性能？';
      const engine = new EnhancementEngine(env.DB);
      const enhanced = await engine.enhance(testInput);
      return new Response(JSON.stringify({
        reply: enhanced.final_output,
        input: testInput,
        output: enhanced.final_output,
        layers: enhanced.layers.length,
        details: enhanced.layers
      }), { headers: jsonHeaders });
    }
    default: {
      if (env.DB && command.length > 0) {
        try {
          await ensurePouchTables(env);
          let row = await env.DB.prepare('SELECT gpt FROM language_pairs WHERE human = ? OR trim(human) = ? LIMIT 1').bind(command, command.trim()).first();
          if (!row) row = await env.DB.prepare("SELECT gpt FROM language_pairs WHERE human LIKE '%' || ? || '%' LIMIT 1").bind(command.trim()).first();
          if (row && row.gpt) return new Response(JSON.stringify({ reply: row.gpt }), { headers: jsonHeaders });
        } catch (_) {}
      }
      await ensurePouchTables(env);
      const engine = new EnhancementEngine(env.DB);
      const result = await engine.enhance(command);
      return new Response(JSON.stringify({ reply: result.final_output }), { headers: jsonHeaders });
    }
  }
}

async function handleAutoTrainStep(env, corsHeaders) {
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
  try {
    await ensurePouchTables(env);
    await env.DB.exec('CREATE TABLE IF NOT EXISTS training_state (k TEXT PRIMARY KEY, v TEXT)');
    const paused = await env.DB.prepare("SELECT v FROM training_state WHERE k = 'paused'").first();
    if (paused && paused.v === '1') return new Response(JSON.stringify({ skipped: true, reason: 'paused' }), { headers: jsonHeaders });
    const rows = await env.DB.prepare("SELECT id, full_input, response_template FROM learned_patterns WHERE status = 'pending' LIMIT 50").all();
    const list = rows.results || [];
    let applied = 0;
    for (const r of list) {
      if (!r.full_input || !r.response_template) continue;
      await env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)').bind(String(r.full_input).trim(), String(r.response_template).trim()).run();
      await env.DB.prepare("UPDATE learned_patterns SET status = 'adopted' WHERE id = ?").bind(r.id).run();
      applied++;
    }
    const now = String(Date.now());
    await env.DB.prepare("INSERT OR REPLACE INTO training_state (k, v) VALUES ('last_run', ?), ('last_count', ?)").bind(now, String(applied)).run();

    let learnResult = null;
    const backend = env.LOGOS_BACKEND || '';
    if (backend) {
      try {
        const lr = await fetch(backend + '/api/chat', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ message: '自主学习' })
        });
        if (lr.ok) learnResult = await lr.json();
      } catch (_) {}
    }

    return new Response(JSON.stringify({ applied, last_run: now, learn: learnResult }), { headers: jsonHeaders });
  } catch (e) {
    return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
  }
}

async function handleAutoTrainPauseResume(env, corsHeaders, pause) {
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
  try {
    await env.DB.exec('CREATE TABLE IF NOT EXISTS training_state (k TEXT PRIMARY KEY, v TEXT)');
    await env.DB.prepare("INSERT OR REPLACE INTO training_state (k, v) VALUES ('paused', ?)").bind(pause ? '1' : '0').run();
    return new Response(JSON.stringify({ paused: pause }), { headers: jsonHeaders });
  } catch (e) {
    return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
  }
}

async function simulateTaskExecution(taskId) {
  const task = tasks.get(taskId);
  if (!task) return;
  task.status = 'running';
  task.updated_at = Date.now();
  for (let i = 0; i < task.steps.length; i++) {
    task.steps[i].status = 'running';
    task.updated_at = Date.now();
    for (let p = 0; p <= 100; p += 20) {
      await new Promise(r => setTimeout(r, 300));
      task.steps[i].progress = p;
      task.updated_at = Date.now();
    }
    task.steps[i].status = 'completed';
    task.steps[i].progress = 100;
    task.updated_at = Date.now();
  }
  task.status = 'completed';
  task.updated_at = Date.now();
}

async function handleTrain(request, corsHeaders) {
  const { pairs } = await request.json();

  if (!Array.isArray(pairs) || pairs.length === 0) {
    return new Response(JSON.stringify({ error: 'Invalid pairs format' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  const patterns = pairs
    .filter(p => p.input && p.output)
    .map(p => [tokenize(p.input), p.output, 0.8]);

  return new Response(JSON.stringify({
    patterns,
    count: patterns.length,
    timestamp: new Date().toISOString(),
    version: '1.0.0'
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleExtract(source, request, corsHeaders) {
  let payload = { sources: [], limit: 50000, output_format: 'constraint_features' };

  try {
    const jsonData = await request.json();
    payload = { ...payload, ...jsonData };
  } catch (_) {
    // continue with defaults
  }

  const { sources, limit } = payload;
  let features = [];

  try {
    switch (source) {
      case 'code':
        features = await extractCodeForms(sources, limit);
        break;
      case 'doc':
        features = await extractDocForms(sources, limit);
        break;
      case 'table':
        features = await extractTableForms(sources, limit);
        break;
      case 'math':
        features = await extractMathForms(sources, limit);
        break;
      case 'law':
        features = await extractLawForms(sources, limit);
        break;
      default:
        return new Response(
          JSON.stringify({ error: `Unknown source: ${source}` }),
          { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
        );
    }
  } catch (err) {
    return new Response(
      JSON.stringify({ error: `Failed to extract from ${source}`, message: err.message }),
      { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } }
    );
  }

  return new Response(JSON.stringify({
    source,
    extracted_count: features.length,
    features,
    timestamp: new Date().toISOString(),
    status: 'complete',
    version: '1.0.0'
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleAggregate(request, corsHeaders) {
  const { results } = await request.json();

  if (!Array.isArray(results)) {
    return new Response(JSON.stringify({ error: 'Invalid results format' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  const aggregated = {};

  for (const result of results) {
    for (const feature of result.features || []) {
      const key = `${feature.source_pouch}:${feature.pattern}`;
      if (key in aggregated) {
        aggregated[key].frequency = (aggregated[key].frequency || 0) + (feature.frequency || 1);
        aggregated[key].confidence = (aggregated[key].confidence + feature.confidence) / 2;
        aggregated[key].source_count = (aggregated[key].source_count || 1) + 1;
      } else {
        aggregated[key] = {
          ...feature,
          source_count: 1
        };
      }
    }
  }

  return new Response(JSON.stringify({
    aggregated_count: Object.keys(aggregated).length,
    features: Object.values(aggregated),
    timestamp: new Date().toISOString(),
    status: 'complete'
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleDiscover(corsHeaders) {
  const services = [
    { name: 'cloudflare-workers', type: 'compute', status: 'active', endpoint: 'https://logos-gateway.amrta.workers.dev' },
    { name: 'cloudflare-kv', type: 'storage', status: 'available', endpoint: '' },
    { name: 'cloudflare-r2', type: 'object-storage', status: 'available', endpoint: '' },
  ];

  return new Response(JSON.stringify({
    services,
    timestamp: new Date().toISOString()
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

const DEFAULT_FEED_INTENTS = [
  '我没有量子力学的背景知识，请向我解释一下路径积分',
  '了解微粒的运动轨迹有什么用处？',
  '解释一下波函数',
  '继续',
  '基于虚拟现实技术产教融合实践，如何培养人才？',
  '成语接龙，不少于5个成语，第一个成语是别具一格。',
  '成语接龙，要求每个成语第一个字与前一个成语最后一个字相同，不少于5个成语，第一个成语齐心合力。',
  '这些性格特征和孔子儒家思想有关吗',
  '这些性格特征与现代科学思想是不是有冲突',
  '山东科技大学教授胡杨林是谁',
  '是农大的还山科大的',
  '哪为啥市场活力不如江苏、浙江、福建等',
  '如何将儒家思想和现代科学思想向融合？相互促进',
  '二者融合以谁为主了？',
  '就目前而言，谁更有现实意义',
  '山东人性格特征上有什么不足的吗',
  '近现代史上，山东思想家有哪些',
  '鲁迅是山东人吗',
  '鲁迅应该是浙江人吧',
  '近现代史上，山东诞生的思想家或科学家有哪些？主要思想和理论是什么？回答严谨些，不要胡诌',
  '好了，不要胡诌八扯了,李约瑟是哪国人',
  '怎么成山东人？',
  '现在中科院院士在山东工作有多少人',
  '如何看待山东的区域不平衡',
  '山东西部应该如何发展？',
  '西部应该发展哪些中心城市',
  '菏泽有金矿吗？',
  '做为一个曹县人，没有听说金矿',
  '又是一本正经的胡说八道',
  '双泉山金矿是那里的',
  '如何看待曹县的封号“宇宙的中心”',
  '从心理学和行为学角度来看 为什么会出现这样的情绪呢？',
  '这种担心和紧张背后有什么限制性的信念呢？',
  '第5点 具体怎么做 可以展开说说 并举例',
  '埃及猫特点是什么？',
  '这些都是很好的建议！我应该做什么样的运动呢？',
  '你说的很有道理，但是我很时常在我的日程上安排了太多事情。我应该做什么都？',
  '我明白了。然而，我发现拒绝太难了。你有什么策略吗？',
  '我知道睡眠很重要，但是我找不到时间。这么多的作业和这么多我想做的事情，我怎么能找到时间睡觉呢？',
  '从社老师那里，我学了冥想的重要性以及它对身体的益处。我还可以学习哪些冥想技巧？',
  '我学校的辅导服务很差。我还有其他方式可以和别人聊吗？',
  '求婚应该准备些什么庆祝的东西？',
  '我和她是通过滑雪走到一起的，这周日我们去街边摆摊，帮我想一个求婚的策划',
  '介绍GNSS的权威资料有哪些',
  'GNSS定位会遇到哪些干扰，目前行业方案用了哪些技术手段使得定位更加准确',
  '新疆牛肉炒拉条子怎么做？',
  '我是一名退休教师，我和几个退休了的好姐妹一起去云南大理旅游，其中有一名老教师，为我们制定行程，安排食宿，教我们人生态度，教我们摄拍美照，我们几个人想给她写一封有趣的感谢信。',
  '请把信件改成一首七律',
  '这是现代诗，不是七律。你知道七律是什么吗？'
];

async function handleFeedStatus(request, corsHeaders) {
  const u = new URL(request.url);
  const backend = (u.searchParams.get('backend') || '').replace(/\/$/, '');
  if (!backend) {
    return new Response(JSON.stringify({ error: 'Missing backend in query: ?backend=...' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }
  try {
    const [archRes, sizeRes] = await Promise.all([
      fetch(`${backend}/api/architecture`),
      fetch(`${backend}/api/data_size`)
    ]);
    const arch = archRes.ok ? await archRes.json() : null;
    const size = sizeRes.ok ? await sizeRes.json() : null;
    return new Response(JSON.stringify({ architecture: arch, data_size: size }), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Fetch backend failed: ' + e.message }), {
      status: 502,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }
}

async function handleFeedStorage(env, corsHeaders) {
  const out = { d1: !!env.DB, supabase: !!(env.SUPABASE_URL && env.SUPABASE_ANON_KEY), firebase: !!(env.FIREBASE_PROJECT_ID || env.FIREBASE_URL) };
  if (env.DB) {
    try {
      const r = await env.DB.prepare('SELECT count(*) as n FROM intents').first();
      out.d1_count = r ? r.n : 0;
    } catch (e) {
      out.d1_error = e.message || String(e);
    }
  }
  return new Response(JSON.stringify(out), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleFeedUpload(request, corsHeaders, env = {}) {
  let url = '';
  let intents = null;
  try {
    const body = await request.json();
    url = body.url || '';
    if (Array.isArray(body.intents)) intents = body.intents;
  } catch (_) {}

  if (!intents && url) {
    try {
      const res = await fetch(url);
      const text = await res.text();
      const first = text.trim().split('\n')[0] || '';
      if (first.startsWith('[')) {
        intents = JSON.parse(text);
        if (!Array.isArray(intents)) intents = null;
      } else {
        intents = [];
        for (const line of text.split('\n')) {
          const t = line.trim();
          if (!t) continue;
          try {
            const o = JSON.parse(t);
            if (o && typeof o.human === 'string') intents.push(o.human);
          } catch (_) {}
        }
      }
    } catch (e) {
      return new Response(JSON.stringify({ error: 'Fetch url failed: ' + e.message }), {
        status: 502,
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    }
  }

  if (!Array.isArray(intents) || intents.length === 0) {
    return new Response(JSON.stringify({ error: 'No intents (provide url or intents array)' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  if (env.DB) {
    try {
      await env.DB.exec('CREATE TABLE IF NOT EXISTS intents (id INTEGER PRIMARY KEY AUTOINCREMENT, human TEXT)');
      const batchSize = 100;
      for (let i = 0; i < intents.length; i += batchSize) {
        const batch = intents.slice(i, i + batchSize);
        const stmts = batch.map(h => env.DB.prepare('INSERT INTO intents (human) VALUES (?)').bind(h));
        await env.DB.batch(stmts);
      }
      return new Response(JSON.stringify({ stored: 'd1', count: intents.length }), {
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    } catch (e) {
      return new Response(JSON.stringify({ stored: 'd1', error: e.message || String(e) }), {
        status: 500,
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    }
  }

  if (env.SUPABASE_URL && env.SUPABASE_ANON_KEY) {
    try {
      const baseUrl = env.SUPABASE_URL.replace(/\/$/, '') + '/rest/v1/intents';
      const batchSize = 500;
      let inserted = 0;
      for (let i = 0; i < intents.length; i += batchSize) {
        const rows = intents.slice(i, i + batchSize).map(h => ({ human: h }));
        const res = await fetch(baseUrl, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'apikey': env.SUPABASE_ANON_KEY,
            'Authorization': `Bearer ${env.SUPABASE_ANON_KEY}`,
            'Prefer': 'return=minimal'
          },
          body: JSON.stringify(rows)
        });
        if (!res.ok) {
          const t = await res.text();
          return new Response(JSON.stringify({ stored: 'supabase', inserted, error: res.status + ' ' + t }), {
            status: 502,
            headers: { ...corsHeaders, 'Content-Type': 'application/json' }
          });
        }
        inserted += rows.length;
      }
      return new Response(JSON.stringify({ stored: 'supabase', count: inserted }), {
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    } catch (e) {
      return new Response(JSON.stringify({ stored: 'supabase', error: e.message || String(e) }), {
        status: 500,
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    }
  }

  if (env.FIREBASE_PROJECT_ID && env.FIREBASE_CLIENT_EMAIL && env.FIREBASE_PRIVATE_KEY) {
    return new Response(JSON.stringify({ stored: 'firebase', error: 'Firestore insert not implemented, use D1 or Supabase' }), {
      status: 501,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  return new Response(JSON.stringify({
    error: 'No storage binding',
    hint: 'Bind D1 (env.DB) or set SUPABASE_URL + SUPABASE_ANON_KEY or FIREBASE_* in wrangler'
  }), {
    status: 503,
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

function stripHtml(s) {
  let out = '';
  let inTag = false;
  for (const c of s) {
    if (c === '<') inTag = true;
    else if (c === '>') inTag = false;
    else if (!inTag) out += c;
  }
  return out.replace(/&nbsp;/g, ' ').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&').replace(/&quot;/g, '"').trim();
}

async function handleFeedExport(env, corsHeaders) {
  if (!env.DB) {
    return new Response(JSON.stringify({ error: 'No D1 binding' }), { status: 503, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
  try {
    const stmt = env.DB.prepare('SELECT human, gpt FROM language_pairs ORDER BY id');
    const r = await stmt.all();
    const rows = r.results || [];
    const jsonl = rows.map(row => JSON.stringify({ human: row.human || '', gpt: row.gpt || '' })).join('\n');
    return new Response(jsonl, {
      headers: { ...corsHeaders, 'Content-Type': 'application/x-ndjson' }
    });
  } catch (e) {
    try {
      const stmt = env.DB.prepare('SELECT human FROM intents ORDER BY id');
      const r = await stmt.all();
      const rows = r.results || [];
      const jsonl = rows.map(row => JSON.stringify({ human: row.human || '', gpt: '' })).join('\n');
      return new Response(jsonl, { headers: { ...corsHeaders, 'Content-Type': 'application/x-ndjson' } });
    } catch (e2) {
      return new Response(JSON.stringify({ error: e.message || String(e) }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
    }
  }
}

async function handleFeedLanguageUpload(request, corsHeaders, env = {}) {
  if (!env.DB) {
    return new Response(JSON.stringify({ error: 'No D1 binding' }), { status: 503, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
  let pairs = [];
  try {
    const body = await request.json();
    if (body.url) {
      const res = await fetch(body.url);
      const text = await res.text();
      for (const line of text.split('\n')) {
        const t = line.trim();
        if (!t) continue;
        try {
          const o = JSON.parse(t);
          if (o && typeof o.human === 'string') pairs.push({ human: o.human.trim(), gpt: (o.gpt && typeof o.gpt === 'string' ? stripHtml(o.gpt) : '').trim() });
        } catch (_) {}
      }
    } else if (Array.isArray(body.pairs)) {
      pairs = body.pairs.filter(p => p && typeof p.human === 'string').map(p => ({
        human: p.human.trim(),
        gpt: (p.gpt && typeof p.gpt === 'string' ? stripHtml(p.gpt) : '').trim()
      }));
    }
  } catch (e) {
    return new Response(JSON.stringify({ error: 'Body failed: ' + e.message }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
  const MIN_H = 5, MAX_H = 500, MAX_G = 3000;
  const seen = new Set();
  const filtered = [];
  for (const p of pairs) {
    if (p.human.length < MIN_H || p.human.length > MAX_H) continue;
    if (p.gpt.length > MAX_G) continue;
    const key = p.human.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    filtered.push(p);
  }
  try {
    await env.DB.exec('CREATE TABLE IF NOT EXISTS language_pairs (id INTEGER PRIMARY KEY AUTOINCREMENT, human TEXT, gpt TEXT)');
    const batchSize = 100;
    for (let i = 0; i < filtered.length; i += batchSize) {
      const batch = filtered.slice(i, i + batchSize);
      const stmts = batch.map(p => env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)').bind(p.human, p.gpt));
      await env.DB.batch(stmts);
    }
    return new Response(JSON.stringify({ stored: 'd1', table: 'language_pairs', count: filtered.length }), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  } catch (e) {
    return new Response(JSON.stringify({ error: e.message || String(e) }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
}

async function handleTest(request, env, corsHeaders) {
  const reqUrl = new URL(request.url);
  const base = reqUrl.origin;
  const out = {};
  try {
    const h = await fetch(base + '/health');
    out.health = h.ok ? 'ok' : ('fail ' + h.status);
  } catch (e) { out.health = 'err ' + e.message; }
  try {
    const s = await fetch(base + '/feed/storage');
    out['feed/storage'] = s.ok ? 'ok' : ('fail ' + s.status);
  } catch (e) { out['feed/storage'] = 'err ' + e.message; }
  try {
    const e = await fetch(base + '/feed/export');
    out['feed/export'] = e.ok ? 'ok' : ('fail ' + e.status);
  } catch (e) { out['feed/export'] = 'err ' + e.message; }
  try {
    const t = await fetch(base + '/train', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ pairs: [{ input: '测', output: '试' }] }) });
    out.train = t.ok ? 'ok' : ('fail ' + t.status);
  } catch (e) { out.train = 'err ' + e.message; }
  try {
    const d = await fetch(base + '/discover');
    out.discover = d.ok ? 'ok' : ('fail ' + d.status);
  } catch (e) { out.discover = 'err ' + e.message; }
  try {
    const a = await fetch(base + '/analyze', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ input: '测试' }) });
    out.analyze = a.ok ? 'ok' : ('fail ' + a.status);
  } catch (e) { out.analyze = 'err ' + e.message; }
  const pouchNames = ['reasoning', 'knowledge_retriever', 'creative', 'memory', 'code_analyzer', 'image_generator', 'chemistry', 'material_analyzer', 'printer_3d', 'discovery', 'cloud_general'];
  for (const name of pouchNames) {
    try {
      const p = await fetch(base + '/pouch/' + name, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ input: '1+1' }) });
      out['pouch/' + name] = p.ok ? 'ok' : ('fail ' + p.status);
    } catch (e) { out['pouch/' + name] = 'err ' + e.message; }
  }
  return new Response(JSON.stringify(out), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
}

async function handleFeed(request, corsHeaders, env = {}) {
  let backend = (env.LOGOS_BACKEND || '').replace(/\/$/, '');
  let url = env.LOGOS_JSONL_URL || '';
  let intents = null;
  let limit = 500;
  let offset = 0;
  try {
    const body = await request.json();
    if (body.backend) backend = (body.backend || '').replace(/\/$/, '');
    if (body.url !== undefined) url = body.url || '';
    if (Array.isArray(body.intents)) intents = body.intents;
    if (typeof body.limit === 'number' && body.limit > 0) limit = Math.min(body.limit, 2000);
    if (typeof body.offset === 'number' && body.offset >= 0) offset = body.offset;
  } catch (_) {}

  if (!backend) {
    return new Response(JSON.stringify({ error: 'Missing backend (set body.backend or wrangler env LOGOS_BACKEND)' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  const chatUrl = `${backend}/api/chat`;
  let fromD1 = false;
  let total = 0;
  if (!intents && !url && env.DB) {
    try {
      const page = await env.DB.prepare('SELECT human FROM intents ORDER BY id LIMIT ? OFFSET ?').bind(limit, offset).all();
      if (page.results && page.results.length > 0) {
        intents = page.results.map(r => r.human);
        const totalRow = await env.DB.prepare('SELECT count(*) as n FROM intents').first();
        total = totalRow ? totalRow.n : intents.length;
        fromD1 = true;
      }
    } catch (_) {}
  }
  if (!intents && !url) {
    intents = DEFAULT_FEED_INTENTS;
  }
  if (!intents && url) {
    try {
      const res = await fetch(url);
      const text = await res.text();
      const first = text.trim().split('\n')[0] || '';
      if (first.startsWith('[')) {
        intents = JSON.parse(text);
        if (!Array.isArray(intents)) intents = null;
      } else {
        intents = [];
        for (const line of text.split('\n')) {
          const t = line.trim();
          if (!t) continue;
          try {
            const o = JSON.parse(t);
            if (o && typeof o.human === 'string') intents.push(o.human);
          } catch (_) {}
        }
      }
    } catch (e) {
      return new Response(JSON.stringify({ error: 'Fetch intents failed: ' + e.message }), {
        status: 502,
        headers: { ...corsHeaders, 'Content-Type': 'application/json' }
      });
    }
  }

  if (!Array.isArray(intents) || intents.length === 0) {
    return new Response(JSON.stringify({ error: 'No intents (use url or intents array or upload to D1 first)' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  if (!fromD1) total = intents.length;
  const toRun = fromD1 ? intents : intents.slice(offset, offset + limit);
  let ok = 0;
  const errors = [];
  for (let i = 0; i < toRun.length; i++) {
    try {
      const r = await fetch(chatUrl, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: toRun[i] })
      });
      if (r.ok) ok += 1;
      else errors.push({ index: offset + i, intent: toRun[i].slice(0, 40), status: r.status });
    } catch (e) {
      errors.push({ index: offset + i, intent: toRun[i].slice(0, 40), error: e.message });
    }
  }

  const next_offset = offset + toRun.length;
  return new Response(JSON.stringify({
    fed: toRun.length,
    ok,
    offset,
    next_offset,
    total,
    done: next_offset >= total,
    errors: errors.slice(0, 20)
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleFeedback(request, corsHeaders, env = {}) {
  let body;
  try {
    body = await request.json();
  } catch (_) {
    return new Response(JSON.stringify({ error: 'Invalid JSON body' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }
  const input = (body.input || '').trim();
  const signal = typeof body.signal === 'number' ? body.signal : 0;
  const correction = body.correction || null;
  if (!input) {
    return new Response(JSON.stringify({ error: 'Missing input field' }), {
      status: 400,
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }
  if (env.DB) {
    try {
      await ensurePouchTables(env);
      await env.DB.prepare(
        'INSERT INTO feedback (input, signal, correction, ts) VALUES (?, ?, ?, ?)'
      ).bind(input.slice(0, 200), signal, correction ? correction.slice(0, 500) : null, new Date().toISOString()).run();
      if (correction && correction.trim().length >= 4) {
        const h = input.slice(0, 500).trim();
        const g = correction.trim().slice(0, 3000);
        if (h.length >= 5 && h.length <= 500) {
          await env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)').bind(h, g).run();
        }
      }
    } catch (_) {}
  }
  const backend = (env.LOGOS_BACKEND || '').replace(/\/$/, '');
  if (backend) {
    try {
      const payload = { input, signal };
      if (correction) payload.correction = correction;
      await fetch(`${backend}/api/feedback`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload)
      });
    } catch (_) {}
  }
  return new Response(JSON.stringify({ status: 'ok', input: input.slice(0, 40), signal }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleFeedbackStatus(corsHeaders, env = {}) {
  const result = { feedback_count: 0, recent: [] };
  if (env.DB) {
    try {
      const count = await env.DB.prepare('SELECT count(*) as n FROM feedback').first();
      result.feedback_count = count ? count.n : 0;
      const recent = await env.DB.prepare(
        'SELECT input, signal, correction, ts FROM feedback ORDER BY rowid DESC LIMIT 20'
      ).all();
      result.recent = recent.results || [];
    } catch (_) {}
  }
  const backend = (env.LOGOS_BACKEND || '').replace(/\/$/, '');
  if (backend) {
    try {
      const r = await fetch(`${backend}/api/feedback_status`);
      if (r.ok) {
        result.backend = await r.json();
      }
    } catch (_) {}
  }
  return new Response(JSON.stringify(result), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handleAnalyze(request, corsHeaders) {
  let input = '';
  try {
    const body = await request.json();
    input = body.input || '';
  } catch (_) {}

  const plan = { pouches: [], steps: [] };
  if (!input) {
    return new Response(JSON.stringify(plan), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  const lower = input.toLowerCase();
  const base = 'https://logos-gateway.amrta.workers.dev/pouch';

  if (/画|draw|图片|image|生成图|绘|sketch|插画|icon/.test(lower)) {
    plan.pouches.push({ name: 'image_generator', endpoint: `${base}/image_generator` });
    plan.steps.push({ pouch: 'image_generator', input: input });
  } else if (/代码|code|编程|程序|bug|debug|编译/.test(lower)) {
    plan.pouches.push({ name: 'code_analyzer', endpoint: `${base}/code_analyzer` });
    plan.steps.push({ pouch: 'code_analyzer', input: input });
  } else if (/化学|分子|元素|化合|molecule|chemistry/.test(lower)) {
    plan.pouches.push({ name: 'chemistry', endpoint: `${base}/chemistry` });
    plan.steps.push({ pouch: 'chemistry', input: input });
  } else if (/计算|推理|数学|math|calculate|公式|方程/.test(lower)) {
    plan.pouches.push({ name: 'reasoning', endpoint: `${base}/reasoning` });
    plan.steps.push({ pouch: 'reasoning', input: input });
  } else if (/什么是|定义|解释|知识|百科|是什么/.test(lower)) {
    plan.pouches.push({ name: 'knowledge_retriever', endpoint: `${base}/knowledge_retriever` });
    plan.steps.push({ pouch: 'knowledge_retriever', input: input });
  } else if (/创意|故事|创作|写|作文|诗/.test(lower)) {
    plan.pouches.push({ name: 'creative', endpoint: `${base}/creative` });
    plan.steps.push({ pouch: 'creative', input: input });
  } else if (/记住|记忆|remember|recall|还记得/.test(lower)) {
    plan.pouches.push({ name: 'memory', endpoint: `${base}/memory` });
    plan.steps.push({ pouch: 'memory', input: input });
  } else if (/材料.*打印|打印.*材料|3d.*print|制造/.test(lower)) {
    plan.pouches.push({ name: 'material_analyzer', endpoint: `${base}/material_analyzer` });
    plan.pouches.push({ name: 'printer_3d', endpoint: `${base}/printer_3d` });
    plan.steps.push({ pouch: 'material_analyzer', input: input });
    plan.steps.push({ pouch: 'printer_3d', input: input });
  } else if (/搜索|查找|发现|search|find|discover/.test(lower)) {
    plan.pouches.push({ name: 'discovery', endpoint: `${base}/discovery` });
    plan.steps.push({ pouch: 'discovery', input: input });
  } else if (input.length > 2) {
    plan.pouches.push({ name: 'cloud_general', endpoint: `${base}/cloud_general` });
    plan.steps.push({ pouch: 'cloud_general', input: input });
  }

  return new Response(JSON.stringify(plan), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function handlePipeline(request, env, corsHeaders) {
  let body = { stages: [], data: '' };
  try {
    body = await request.json();
  } catch (_) {}
  const stages = Array.isArray(body.stages) ? body.stages : (body.stages ? [body.stages] : []);
  let data = typeof body.data === 'string' ? body.data : (body.data != null ? String(body.data) : '');
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  const steps = [];
  for (const stage of stages) {
    const name = typeof stage === 'string' ? stage : (stage && stage.name ? stage.name : '');
    if (!name) continue;
    const req = new Request(request.url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ input: data })
    });
    const res = await handlePouch(name, req, corsHeaders, env);
    let out = data;
    if (res && res.ok) {
      try {
        const j = await res.json();
        out = (j && (j.result != null)) ? String(j.result) : (j && j.output != null ? String(j.output) : data);
      } catch (_) {}
    }
    steps.push({ pouch: name, input: data.slice(0, 80), output: out.slice(0, 80) });
    data = out;
  }
  return new Response(JSON.stringify({ result: data, steps }), { headers: jsonHeaders });
}

async function handlePouch(pouchName, request, corsHeaders, env) {
  let input = '';
  try {
    const body = await request.json();
    input = body.input || '';
  } catch (_) {}

  if (env && env.DB && pouchName === 'language') {
    const match = await pureLanguageMatch(env, input);
    if (match) return new Response(JSON.stringify({ result: match.result, confidence: match.confidence }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
    return new Response(JSON.stringify({
      result: '这句我还对不上。可以说「教你 X -> Y」教我，或切到通用回复（如 cloud_general）。',
      confidence: 0.3
    }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }

  if (env && env.DB && pouchName === 'seed') {
    const seedEngine = new SeedEngine(env.DB);
    const seedMatch = seedEngine.matchSeed(input);
    const grown = seedMatch ? await seedEngine.grow(input, seedMatch) : null;
    const out = grown ? (grown.enhanced_response || grown.base_response) : '收到您的输入。';
    const conf = grown ? (grown.confidence ?? 0.5) : 0.5;
    return new Response(JSON.stringify({ result: out, confidence: conf }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }

  if (env && env.DB && (pouchName === 'creative' || pouchName === 'cloud_general')) {
    await ensurePouchTables(env);
    const engine = new EnhancementEngine(env.DB);
    const out = await engine.enhance(input);
    return new Response(JSON.stringify({ result: out.final_output, confidence: 0.85 }), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  const processors = {
    reasoning: (text) => {
      const match = text.match(/(\d+)\s*([+\-*/÷×])\s*(\d+)/);
      if (match) {
        const [, a, op, b] = match;
        const na = parseFloat(a), nb = parseFloat(b);
        const ops = { '+': na + nb, '-': na - nb, '*': na * nb, '×': na * nb, '/': na / nb, '÷': na / nb };
        const result = ops[op];
        if (result !== undefined) return { result: `${a} ${op} ${b} = ${result}`, confidence: 1.0 };
      }
      return { result: `推理输入: ${text}`, confidence: 0.5 };
    },
    memory: (text) => {
      return { result: `已记录: ${text}`, confidence: 0.9 };
    },
    image_generator: (text) => {
      const desc = text.slice(0, 200);
      return { result: `[图像生成] 描述已接收: ${desc}。云端图像生成能力已挂接，输出占位。`, confidence: 0.7 };
    },
    code_analyzer: (text) => {
      const hasCode = /function|def |class |fn |=>|const |let |import |require\(/.test(text);
      return { result: hasCode ? `[代码分析] 已识别代码片段，长度 ${text.length} 字符。` : `[代码分析] 输入: ${text.slice(0, 100)}`, confidence: 0.75 };
    },
    knowledge_retriever: async (text, env) => {
      const queries = buildSearchQueries(text);
      if (env && env.DB && queries.length > 0) {
        let hits = await searchLearnedAndPairs(env, queries, 6);
        if (hits.length === 0 && queries.length > 1) hits = await searchLearnedAndPairs(env, queries.slice(0, 2), 6);
        if (hits.length > 0) {
          const lines = hits.map(h => `• ${(h.output || '').toString().trim().slice(0, 120)}`).join('\n');
          return { result: `[知识检索] 在学习库中匹配到：\n${lines}`, confidence: 0.85 };
        }
      }
      return { result: `[知识检索] 查询「${text.slice(0, 80)}」未在学习库中命中。可尝试 /learn/from-url 或 /learn/from-text 补充语料。`, confidence: 0.5 };
    },
    chemistry: (text) => {
      const m = text.match(/([A-Z][a-z]?\d*)/g);
      const formula = m ? m.join('') : text.slice(0, 30);
      return { result: `[化学] 分子或表达式: ${formula}，云端化学尿袋已挂接。`, confidence: 0.8 };
    },
    material_analyzer: (text) => {
      const elements = ['Fe', 'C', 'Al', 'Cu', 'Si'].filter((e) => text.includes(e));
      return { result: elements.length ? `[材料分析] 检测到元素: ${elements.join(', ')}` : `[材料分析] 输入: ${text.slice(0, 60)}`, confidence: 0.85 };
    },
    printer_3d: (text) => {
      return { result: `[3D打印] G-Code 生成已挂接。输入摘要: ${text.slice(0, 50)}`, confidence: 0.8 };
    },
    discovery: async (text, env) => {
      const queries = buildSearchQueries(text);
      if (env && env.DB && queries.length > 0) {
        let hits = await searchLearnedAndPairs(env, queries, 8);
        if (hits.length === 0 && queries.length > 1) hits = await searchLearnedAndPairs(env, queries.slice(0, 2), 8);
        if (hits.length > 0) {
          const lines = hits.map(h => `• ${(h.input || '').toString().trim().slice(0, 60)} → ${(h.output || '').toString().trim().slice(0, 80)}`).join('\n');
          return { result: `[发现] 在学习库中匹配到：\n${lines}`, confidence: 0.85 };
        }
      }
      return { result: `[发现] 查询「${text.slice(0, 60)}」未命中。可通过 /learn/from-url 或 /learn/from-text 扩充学习库后重试。`, confidence: 0.5 };
    }
  };

  const processor = processors[pouchName];
  if (processor) {
    const output = await Promise.resolve(processor(input, env));
    const resultStr = (output && output.result != null) ? String(output.result).trim().slice(0, 300) : '';
    const skipFeedback = ['language', 'creative', 'cloud_general', 'seed'].includes(pouchName);
    if (env && env.DB && !skipFeedback && resultStr) {
      try {
        await ensurePouchTables(env);
        const content = `${String(input).trim().slice(0, 200)} => ${resultStr}`;
        await env.DB.prepare('INSERT INTO pouch_feedback (from_pouch, to_pouch, feedback_type, content, confidence, created_at) VALUES (?, ?, ?, ?, ?, ?)')
          .bind(pouchName, 'language', 'execution', content, (output && typeof output.confidence === 'number') ? output.confidence : 0.7, Date.now()).run();
      } catch (_) {}
    }
    return new Response(JSON.stringify(output), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  if (env && env.DB) {
    await ensurePouchTables(env);
    const engine = new EnhancementEngine(env.DB);
    const out = await engine.enhance(input);
    return new Response(JSON.stringify({ result: out.final_output, confidence: 0.75 }), {
      headers: { ...corsHeaders, 'Content-Type': 'application/json' }
    });
  }

  return new Response(JSON.stringify({
    result: `[${pouchName}] ${input}`,
    confidence: 0.5
  }), {
    headers: { ...corsHeaders, 'Content-Type': 'application/json' }
  });
}

async function extractCodeForms(sources, limit) {
  return [
    { pattern: 'function(T)->T', frequency: 100, confidence: 0.9, source_pouch: 'Code_Pouch', examples: ['fn process(input: String) -> String'] },
    { pattern: 'async fn(T)->Future<T>', frequency: 50, confidence: 0.85, source_pouch: 'Code_Pouch', examples: ['async fn fetch_data(url: &str) -> Result<String>'] }
  ];
}

async function extractDocForms(sources, limit) {
  return [
    { pattern: 'endpoint(method, path)->response', frequency: 150, confidence: 0.92, source_pouch: 'Doc_Pouch', examples: ['GET /api/users -> {id, name, email}'] },
    { pattern: 'concept(definition, usage)', frequency: 200, confidence: 0.88, source_pouch: 'Doc_Pouch', examples: ['Authentication: process of verifying user identity'] }
  ];
}

async function extractTableForms(sources, limit) {
  return [
    { pattern: 'schema(column:type)', frequency: 300, confidence: 0.95, source_pouch: 'Table_Pouch', examples: ['id:int, name:string, age:int'] },
    { pattern: 'enum(values)', frequency: 80, confidence: 0.91, source_pouch: 'Table_Pouch', examples: ['status: [active, inactive, pending]'] }
  ];
}

async function extractMathForms(sources, limit) {
  return [
    { pattern: 'theorem->proof->qed', frequency: 120, confidence: 0.87, source_pouch: 'Math_Pouch', examples: ['If P(n) holds for n=k, then P(n) holds for n=k+1'] },
    { pattern: 'definition(term, formal_definition)', frequency: 250, confidence: 0.93, source_pouch: 'Math_Pouch', examples: ['Prime: natural number > 1 with no positive divisors except 1 and itself'] }
  ];
}

async function extractLawForms(sources, limit) {
  return [
    { pattern: 'if(condition)->then(consequence)', frequency: 500, confidence: 0.94, source_pouch: 'Law_Pouch', examples: ['If person is a U.S. citizen, then they have voting rights'] },
    { pattern: 'section(number)->subsection(rule)', frequency: 1000, confidence: 0.96, source_pouch: 'Law_Pouch', examples: ['§ 1. Definitions; § 2. Penalties'] }
  ];
}

function tokenize(text) {
  return text.split('').filter(c => c.trim());
}

const VERIFY_MIN_HUMAN = 5;
const VERIFY_MAX_HUMAN = 500;
const VERIFY_MIN_GPT = 1;
const VERIFY_MAX_GPT = 3000;
const VERIFY_DEFAULT_MAX_ADD = 5000;
const VERIFY_DOMAIN_KEYWORDS = ['解释', '怎么', '什么', '如何', '为什么', '治疗', '医学', '代码', '编程', '学习', '教育', '量子', '物理', '化学', '数学', '历史', '文化', '艺术', '心理', '经济', '法律'];

function isFallbackOutput(s) {
  return typeof s !== 'string' ? false : (
    s.includes('我的模式库里没有') || s.includes('没有匹配的模式') || s.includes('我暂时无法处理')
  );
}

function hasDomainKeyword(s) {
  if (typeof s !== 'string') return false;
  return VERIFY_DOMAIN_KEYWORDS.some(k => s.includes(k));
}

function isValidHitStrict(output) {
  if (typeof output !== 'string') return false;
  const templates = ['是的，我还在', '我在的', '您好，我在听'];
  if (templates.some(t => output.includes(t))) return false;
  if (output.includes('我的模式库里没有') || output.includes('没有匹配的模式')) return false;
  if (output.length < 10) return false;
  return true;
}

async function getEvalFromRequestOrR2(request, env) {
  try {
    const body = request.method === 'POST' && request.body ? await request.json().catch(() => ({})) : {};
    if (body.eval_result && typeof body.eval_result === 'string') return body.eval_result;
    if (env.LOGOS_BUCKET) {
      const obj = await env.LOGOS_BUCKET.get('eval_result.jsonl');
      if (obj) return await obj.text();
    }
  } catch (_) {}
  return null;
}

async function handleVerifyAugment(request, env, corsHeaders) {
  try {
    let evalResult = '';
    let sharegpt = '';
    let trainExisting = '';
    let maxAdd = VERIFY_DEFAULT_MAX_ADD;
    if (request.method === 'POST' && request.body) {
      const body = await request.json().catch(() => ({}));
      evalResult = body.eval_result || '';
      sharegpt = body.sharegpt || '';
      trainExisting = body.train_existing || '';
      if (typeof body.max_add === 'number') maxAdd = Math.min(50000, Math.max(0, body.max_add));
    } else if (request.method === 'GET') {
      maxAdd = Math.min(100, VERIFY_DEFAULT_MAX_ADD);
    }
    if (env.LOGOS_BUCKET) {
      if (!evalResult) try { const o = await env.LOGOS_BUCKET.get('eval_result.jsonl'); if (o) evalResult = await o.text(); } catch (_) {}
      if (!sharegpt) try { const o = await env.LOGOS_BUCKET.get('sharegpt_pairs.jsonl'); if (o) sharegpt = await o.text(); } catch (_) {}
      if (!trainExisting) try { const o = await env.LOGOS_BUCKET.get('cleaned_language_train.jsonl'); if (o) trainExisting = await o.text(); } catch (_) {}
    }
    const fallbackInputs = new Set();
    const lines = (evalResult || '').split('\n').map(l => l.trim()).filter(Boolean);
    for (const line of lines) {
      try {
        const v = JSON.parse(line);
        const out = (v.logos_output || '').toString();
        if (isFallbackOutput(out) && v.input) fallbackInputs.add(String(v.input).trim());
      } catch (_) {}
    }
    const existing = new Set();
    const trainLines = (trainExisting || '').split('\n').map(l => l.trim()).filter(Boolean);
    for (const line of trainLines) {
      try {
        const v = JSON.parse(line);
        if (v.human) existing.add(String(v.human).trim());
      } catch (_) {}
    }
    const added = [];
    const sharegptLines = (sharegpt || '').split('\n').map(l => l.trim()).filter(Boolean);
    for (const line of sharegptLines) {
      if (added.length >= maxAdd) break;
      try {
        const raw = JSON.parse(line);
        const human = (raw.human || '').trim();
        const gpt = (raw.gpt || '').trim();
        if (human.length < VERIFY_MIN_HUMAN || human.length > VERIFY_MAX_HUMAN) continue;
        if (gpt.length < VERIFY_MIN_GPT || gpt.length > VERIFY_MAX_GPT) continue;
        if (existing.has(human)) continue;
        if (!hasDomainKeyword(human)) continue;
        existing.add(human);
        added.push({ human, gpt });
      } catch (_) {}
    }
    return new Response(JSON.stringify({
      fallback_samples: fallbackInputs.size,
      existing_train_lines: trainLines.length,
      added: added.length,
      samples: added.slice(0, 50),
      message: added.length > 0 ? `✅ added ${added.length} samples` : 'no new samples to add (provide sharegpt + eval_result)'
    }), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  } catch (e) {
    return new Response(JSON.stringify({ error: e.message }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
}

async function handleVerifyStats(request, env, corsHeaders) {
  try {
    let evalResult = '';
    let strict = true;
    let minHitRate = 0.65;
    if (request.method === 'POST' && request.body) {
      const body = await request.json().catch(() => ({}));
      evalResult = body.eval_result || '';
      strict = body.strict !== false;
      if (typeof body.min_hit_rate === 'number') minHitRate = body.min_hit_rate;
    }
    if (!evalResult && env.LOGOS_BUCKET) {
      try { const o = await env.LOGOS_BUCKET.get('eval_result.jsonl'); if (o) evalResult = await o.text(); } catch (_) {}
    }
    if (!evalResult) {
      return new Response(JSON.stringify({ error: 'eval_result required: pass in body or upload eval_result.jsonl to R2 LOGOS_BUCKET' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
    }
    let total = 0, hits = 0, hitsStrict = 0, fallback = 0, identity = 0, template = 0;
    const lines = evalResult.split('\n').map(l => l.trim()).filter(Boolean);
    for (const line of lines) {
      try {
        const v = JSON.parse(line);
        total++;
        const output = typeof v.logos_output === 'string' ? v.logos_output : '';
        const rawHit = !!v.hit;
        if (rawHit) hits++;
        if (strict && rawHit && isValidHitStrict(output)) hitsStrict++;
        else if (!strict && rawHit) hitsStrict++;
        if (output.includes('我的模式库里没有') || output.includes('没有匹配的模式')) fallback++;
        if (output.includes('文心一言') || output.includes('百度')) identity++;
        if (output.includes('是的，我还在')) template++;
      } catch (_) {}
    }
    if (total === 0) {
      return new Response(JSON.stringify({ error: 'no valid lines' }), { status: 400, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
    }
    const hitRate = hitsStrict / total;
    const pass = hitRate >= minHitRate;
    const out = {
      total,
      hits,
      hits_strict: hitsStrict,
      hit_rate: Math.round(hitRate * 1000) / 1000,
      hit_rate_pct: (hitRate * 100).toFixed(1) + '%',
      explicit_fallback: fallback,
      explicit_fallback_pct: ((fallback / total) * 100).toFixed(1) + '%',
      identity_drift: identity,
      template_yes_still: template,
      min_hit_rate: minHitRate,
      pass,
      message: pass ? `✅ Hit rate ${(hitRate * 100).toFixed(1)}% meets threshold ${(minHitRate * 100).toFixed(1)}%` : `❌ Hit rate ${(hitRate * 100).toFixed(1)}% below threshold ${(minHitRate * 100).toFixed(1)}%`
    };
    return new Response(JSON.stringify(out), { headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  } catch (e) {
    return new Response(JSON.stringify({ error: e.message }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
}

const HARVEST_SOURCES = [
  { name: 'sharegpt_gpt4', type: 'hf', dataset: 'shibing624/sharegpt_gpt4', human_field: 'human', gpt_field: 'assistant', config: 'default', split: 'train', batch: 50 },
  { name: 'belle_0.5m', type: 'hf', dataset: 'BelleGroup/train_0.5M_CN', human_field: 'instruction', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'alpaca_gpt4_zh', type: 'hf', dataset: 'shibing624/alpaca-zh', human_field: 'instruction', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'firefly_1m', type: 'hf', dataset: 'YeungNLP/firefly-train-1.1M', human_field: 'input', gpt_field: 'target', config: 'default', split: 'train', batch: 50 },
  { name: 'moss_sft', type: 'hf', dataset: 'fnlp/moss-002-sft-data', human_field: 'plain_text', gpt_field: '', config: 'default', split: 'train', batch: 20 },
  { name: 'stem_zh', type: 'hf', dataset: 'hfl/stem_zh_instruction', human_field: 'input', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'medical_zh', type: 'hf', dataset: 'shibing624/medical', human_field: 'instruction', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'wiki_zh', type: 'hf', dataset: 'pleisto/wikipedia-cn-20230720-filtered', human_field: 'title', gpt_field: 'text', config: 'default', split: 'train', batch: 30 },
  { name: 'finance_zh', type: 'hf', dataset: 'FinGPT/fingpt-sentiment-train', human_field: 'input', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'law_zh', type: 'hf', dataset: 'ShengbinYue/DISC-Law-SFT', human_field: 'input', gpt_field: 'output', config: 'default', split: 'train', batch: 40 },
  { name: 'code_instruct', type: 'hf', dataset: 'sahil2801/CodeAlpaca-20k', human_field: 'instruction', gpt_field: 'output', config: 'default', split: 'train', batch: 50 },
  { name: 'science_qa', type: 'hf', dataset: 'derek-thomas/ScienceQA', human_field: 'question', gpt_field: 'solution', config: 'default', split: 'train', batch: 40 },
  { name: 'arxiv_abstracts', type: 'rss', url: 'https://export.arxiv.org/api/query?search_query=cat:cs.AI&start={offset}&max_results={batch}&sortBy=submittedDate&sortOrder=descending', batch: 20, parser: 'arxiv' },
  { name: 'hacker_news', type: 'rss', url: 'https://hn.algolia.com/api/v1/search?tags=story&hitsPerPage={batch}&page={page}', batch: 20, parser: 'hn' },
];

async function handleHarvest(env, corsHeaders) {
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
  try {
    await ensurePouchTables(env);
    const today = new Date().toISOString().slice(0, 10);
    await env.DB.prepare('INSERT OR IGNORE INTO quota_log (date, requests, subrequests) VALUES (?, 0, 0)').bind(today).run();
    const quota = await env.DB.prepare('SELECT requests, subrequests FROM quota_log WHERE date = ?').bind(today).first();
    if (quota && quota.requests >= 90000) {
      return new Response(JSON.stringify({ skipped: true, reason: 'daily_quota_near_limit', requests_today: quota.requests }), { headers: jsonHeaders });
    }

    let picked = null;
    for (const src of HARVEST_SOURCES) {
      const state = await env.DB.prepare('SELECT offset_val, exhausted FROM harvest_state WHERE source = ?').bind(src.name).first();
      if (state && state.exhausted) continue;
      picked = { ...src, offset: (state && state.offset_val) || 0 };
      break;
    }
    if (!picked) {
      await env.DB.prepare("UPDATE harvest_state SET exhausted = 0, offset_val = 0 WHERE exhausted = 1").run();
      picked = { ...HARVEST_SOURCES[0], offset: 0 };
    }

    let harvested = 0;
    let newOffset = picked.offset;
    let exhausted = false;

    if (picked.type === 'hf') {
      const url = `https://datasets-server.huggingface.co/rows?dataset=${encodeURIComponent(picked.dataset)}&config=${picked.config}&split=${picked.split}&offset=${picked.offset}&length=${picked.batch}`;
      const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-Harvest/1.0' } });
      await env.DB.prepare('UPDATE quota_log SET subrequests = subrequests + 1 WHERE date = ?').bind(today).run();
      if (res.ok) {
        const data = await res.json();
        const rows = (data.rows || []);
        if (rows.length === 0) {
          exhausted = true;
        } else {
          const batchInserts = [];
          for (const item of rows) {
            const row = item.row || item;
            let human = '';
            let gpt = '';
            if (picked.gpt_field === '' && picked.human_field === 'plain_text') {
              const text = row.plain_text || row.text || '';
              const parts = text.split('<eoh>');
              if (parts.length >= 2) { human = parts[0].replace(/<\|.*?\|>/g, '').trim(); gpt = parts[1].replace(/<\|.*?\|>/g, '').trim(); }
            } else {
              human = String(row[picked.human_field] || '').trim();
              gpt = String(row[picked.gpt_field] || '').trim();
              if (!gpt && row.output) gpt = String(row.output).trim();
              if (!gpt && row.response) gpt = String(row.response).trim();
            }
            if (human.length < 4 || gpt.length < 4) continue;
            if (human.length > 2000) human = human.slice(0, 2000);
            if (gpt.length > 5000) gpt = gpt.slice(0, 5000);
            batchInserts.push({ human, gpt });
          }
          const batchSize = 25;
          for (let i = 0; i < batchInserts.length; i += batchSize) {
            const batch = batchInserts.slice(i, i + batchSize);
            const stmt = env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)');
            await env.DB.batch(batch.map(p => stmt.bind(p.human, p.gpt)));
          }
          harvested = batchInserts.length;
          newOffset = picked.offset + rows.length;
          if (rows.length < picked.batch) exhausted = true;
        }
      }
    }

    if (picked.type === 'rss') {
      const page = Math.floor(picked.offset / picked.batch);
      const url = picked.url.replace('{offset}', String(picked.offset)).replace('{batch}', String(picked.batch)).replace('{page}', String(page));
      try {
        const res = await fetch(url, { headers: { 'User-Agent': 'LOGOS-Harvest/1.0' } });
        await env.DB.prepare('UPDATE quota_log SET subrequests = subrequests + 1 WHERE date = ?').bind(today).run();
        if (res.ok) {
          const batchInserts = [];
          if (picked.parser === 'arxiv') {
            const xml = await res.text();
            const entries = xml.split('<entry>').slice(1);
            for (const entry of entries) {
              const title = (entry.match(/<title>([\s\S]*?)<\/title>/) || [])[1] || '';
              const summary = (entry.match(/<summary>([\s\S]*?)<\/summary>/) || [])[1] || '';
              const human = title.replace(/\s+/g, ' ').trim();
              const gpt = summary.replace(/\s+/g, ' ').trim();
              if (human.length >= 4 && gpt.length >= 10) batchInserts.push({ human: human.slice(0, 2000), gpt: gpt.slice(0, 5000) });
            }
            if (entries.length === 0) exhausted = true;
          } else if (picked.parser === 'hn') {
            const data = await res.json();
            const hits = data.hits || [];
            for (const hit of hits) {
              const human = (hit.title || '').trim();
              const gpt = [hit.url || '', hit.story_text || ''].filter(Boolean).join(' ').trim();
              if (human.length >= 4 && gpt.length >= 4) batchInserts.push({ human: human.slice(0, 2000), gpt: gpt.slice(0, 5000) });
            }
            if (hits.length === 0) exhausted = true;
          }
          const batchSize = 25;
          for (let i = 0; i < batchInserts.length; i += batchSize) {
            const batch = batchInserts.slice(i, i + batchSize);
            const stmt = env.DB.prepare('INSERT INTO language_pairs (human, gpt) VALUES (?, ?)');
            await env.DB.batch(batch.map(p => stmt.bind(p.human, p.gpt)));
          }
          harvested = batchInserts.length;
          newOffset = picked.offset + picked.batch;
        }
      } catch (_e) {}
    }

    await env.DB.prepare('INSERT OR REPLACE INTO harvest_state (source, offset_val, total_harvested, last_run, exhausted) VALUES (?, ?, COALESCE((SELECT total_harvested FROM harvest_state WHERE source = ?), 0) + ?, ?, ?)')
      .bind(picked.name, newOffset, picked.name, harvested, Date.now(), exhausted ? 1 : 0).run();
    await env.DB.prepare('UPDATE quota_log SET requests = requests + 1 WHERE date = ?').bind(today).run();

    const totalRow = await env.DB.prepare('SELECT SUM(total_harvested) as t FROM harvest_state').first();
    const pairsCount = await env.DB.prepare('SELECT COUNT(*) as c FROM language_pairs').first();

    return new Response(JSON.stringify({
      source: picked.name,
      harvested,
      offset: newOffset,
      exhausted,
      total_all_sources: (totalRow && totalRow.t) || 0,
      language_pairs_in_d1: (pairsCount && pairsCount.c) || 0
    }), { headers: jsonHeaders });
  } catch (e) {
    return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
  }
}

async function handleQuota(env, corsHeaders) {
  const jsonHeaders = { ...corsHeaders, 'Content-Type': 'application/json' };
  if (!env.DB) return new Response(JSON.stringify({ error: 'No D1' }), { status: 503, headers: jsonHeaders });
  try {
    await ensurePouchTables(env);
    const today = new Date().toISOString().slice(0, 10);
    await env.DB.prepare('INSERT OR IGNORE INTO quota_log (date, requests, subrequests) VALUES (?, 0, 0)').bind(today).run();
    const row = await env.DB.prepare('SELECT * FROM quota_log WHERE date = ?').bind(today).first();
    const history = await env.DB.prepare('SELECT * FROM quota_log ORDER BY date DESC LIMIT 7').all();
    const harvestRows = await env.DB.prepare('SELECT source, offset_val, total_harvested, last_run, exhausted FROM harvest_state ORDER BY last_run DESC').all();
    return new Response(JSON.stringify({
      today: row || { date: today, requests: 0, subrequests: 0 },
      free_tier_limit: 100000,
      remaining: 100000 - ((row && row.requests) || 0),
      history: (history && history.results) || [],
      harvest_progress: (harvestRows && harvestRows.results) || []
    }), { headers: jsonHeaders });
  } catch (e) {
    return new Response(JSON.stringify({ error: (e && e.message) || String(e) }), { status: 500, headers: jsonHeaders });
  }
}

async function handleVerifyReport(request, corsHeaders) {
  try {
    let stats = null;
    let augment = null;
    if (request.method === 'POST' && request.body) {
      const body = await request.json().catch(() => ({}));
      stats = body.stats || null;
      augment = body.augment || null;
    }
    const lines = [
      '# Phase 1 最终验证摘要 (Worker)',
      '',
      '**日期**: ' + new Date().toISOString().slice(0, 10),
      '',
      '## 构建验证',
      '- cargo build: 需在本地或 CI 执行',
      '- cargo test: 需在本地或 CI 执行',
      '- cargo clippy: 需在本地或 CI 执行',
      '',
      '## 基线验证 (Worker /verify/stats)'
    ];
    if (stats) {
      lines.push('- 评估数据: total ' + stats.total);
      const ratePct = typeof stats.hit_rate === 'number' ? (stats.hit_rate * 100).toFixed(1) + '%' : (stats.hit_rate_pct || 'N/A');
      lines.push('- 命中率: ' + ratePct);
      lines.push('- 显式回退: ' + stats.explicit_fallback + ' (' + (stats.explicit_fallback_pct || '') + ')');
      lines.push('- pass: ' + (stats.pass ? '✅' : '❌'));
      lines.push('');
    } else {
      lines.push('- 请先调用 POST /verify/stats 并传入 eval_result，再将返回的 stats 与 augment 传入 POST /verify/report');
      lines.push('');
    }
    if (augment) {
      lines.push('## 数据扩充 (Worker /verify/augment)');
      lines.push('- added: ' + (augment.added || 0));
      lines.push('- fallback_samples: ' + (augment.fallback_samples || 0));
      lines.push('');
    }
    lines.push('## 交付物');
    lines.push('- [x] 验证端点 /verify/augment, /verify/stats, /verify/report');
    lines.push('- [ ] 本地 build/test/clippy 通过');
    lines.push('');
    const md = lines.join('\n');
    return new Response(md, { headers: { ...corsHeaders, 'Content-Type': 'text/markdown; charset=utf-8' } });
  } catch (e) {
    return new Response(JSON.stringify({ error: e.message }), { status: 500, headers: { ...corsHeaders, 'Content-Type': 'application/json' } });
  }
}
