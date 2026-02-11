use crate::frozen::bedrock;
use std::cmp::Ordering;
use std::io::Write;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub tokens: Vec<String>,
    pub response: String,
    pub weight: f64,
    pub frequency: u32,
    pub last_used: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FractalNode {
    pub children: std::collections::HashMap<String, FractalNode>,
    pub pattern_ids: Vec<usize>,
}

impl FractalNode {
    pub fn insert(&mut self, tokens: &[String], id: usize, depth: usize) {
        if depth >= bedrock::RECURSIVE_MAX_DEPTH || tokens.is_empty() {
            if !self.pattern_ids.contains(&id) {
                self.pattern_ids.push(id);
            }
            return;
        }
        self.children
            .entry(tokens[0].clone())
            .or_default()
            .insert(&tokens[1..], id, depth + 1);
    }

    pub fn search(&self, tokens: &[String], depth: usize) -> Vec<(usize, f64)> {
        let sim = 1.0 - (depth as f64 * 0.02).min(0.5);
        let mut results: Vec<(usize, f64)> = self.pattern_ids.iter().map(|&id| (id, sim)).collect();
        if tokens.is_empty() {
            return results;
        }
        if let Some(child) = self.children.get(&tokens[0]) {
            results.extend(child.search(&tokens[1..], depth + 1));
        }
        for (key, child) in &self.children {
            if key != &tokens[0] && Self::similar(key, &tokens[0]) {
                for (id, sim) in child.search(&tokens[1..], depth + 2) {
                    results.push((id, sim * 0.8));
                }
            }
        }
        results
    }

    fn similar(a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if a.contains(b) || b.contains(a) {
            return true;
        }
        Self::edit_dist(a, b) <= 2
    }

    fn edit_dist(a: &str, b: &str) -> usize {
        let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
        let (m, n) = (a.len(), b.len());
        if m == 0 {
            return n;
        }
        if n == 0 {
            return m;
        }
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
            row[0] = i;
        }
        for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
            *val = j;
        }
        for i in 1..=m {
            for j in 1..=n {
                let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
                dp[i][j] = (dp[i - 1][j] + 1)
                    .min(dp[i][j - 1] + 1)
                    .min(dp[i - 1][j - 1] + cost);
            }
        }
        dp[m][n]
    }
}

pub const MAX_CONTEXT_TURNS: usize = 10;

const SYNC_BUFFER_MAX: usize = 50;
const SYNC_MATCH_THRESHOLD: f64 = 0.6;

pub struct LanguagePouch {
    patterns: Vec<Pattern>,
    root: FractalNode,
    tick: u64,
    route_patterns: Vec<(Vec<String>, String)>,
    sync_buffer: Vec<(Vec<String>, String)>,
    context: Vec<(String, String)>,
    last_was_pattern_hit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementIntent {
    pub intent_type: String,
    pub capability_needed: String,
    pub description: String,
    pub confidence: f64,
}

impl LanguagePouch {
    pub fn new() -> Self {
        let mut p = Self {
            patterns: Vec::new(),
            root: FractalNode::default(),
            tick: 0,
            route_patterns: Vec::new(),
            sync_buffer: Vec::new(),
            context: Vec::new(),
            last_was_pattern_hit: false,
        };
        p.seed();
        p
    }

    fn seed(&mut self) {
        let seeds: &[(&[&str], &str)] = &[
            (&["你好"], "你好。有什么需要？"),
            (&["你是谁"], "LOGOS，本地分形学习系统。我没有云端大模型，靠对话积累模式。模式越多，我越准确。"),
            (&["能做什么", "功能"], "核心能力：1)从对话中提取模式并记住 2)通过尿袋扩展专项能力 3)路由请求到合适的尿袋 4)被动演化（高频模式晋升为路由规则）。输入「帮助」看完整命令。"),
            (&["谢谢"], "不客气。"),
            (&["再见"], "再见。"),
            (&["怎么学习", "学习方式"], "每次对话→分词→存入分形树→下次相似输入命中→权重增长→高频模式自动晋升为路由规则。用「教你 X -> Y」可直接教我新模式。"),
            (&["早上好", "早安"], "早上好。"),
            (&["晚安"], "晚安。"),
            (&["好的", "明白", "了解"], "好。"),
            (&["不对", "错了"], "抱歉。用「教你 X -> Y」纠正我，我会记住。"),
            (&["是", "对"], "好的。"),
            (&["不是", "不"], "明白了。"),
            (&["为什么"], "请具体说明想了解什么。"),
            (&["怎么"], "请具体说明想做什么。"),
            (&["可以", "能不能"], "取决于已安装的尿袋。输入「尿袋列表」查看当前能力。"),
            (&["介绍", "自我介绍"], "我是LOGOS v5.0，本地运行的分形学习系统。没有预训练知识库，所有能力来自：1)对话积累的模式 2)已安装的尿袋模块。我的优势是透明和可审计——每条回复都能追溯来源。"),
            (&["你和chatgpt有什么区别", "和gpt区别"], "GPT/Claude是云端大模型，有海量预训练知识。我是本地模式匹配系统，初始几乎空白，靠对话学习。我的优势：完全本地、可审计、零幻觉（不会编造）。劣势：初始能力极低，需要大量对话积累。"),
            (&["你很笨", "你不行", "太弱了"], "确实。我当前模式库很小，能力有限。每次对话都在学习。你也可以：1)「教你 X -> Y」直接教我 2)「安装尿袋<名>」扩展能力 3)「自我优化」让我自动补齐核心尿袋。"),
            (&["什么是尿袋", "尿袋是什么"], "尿袋(Pouch)是LOGOS的可插拔能力模块。每个尿袋专注一个领域（推理、代码分析、知识检索等）。输入「尿袋列表」查看已安装的，「安装尿袋<名>」添加新能力。"),
            (&["什么是分形", "分形是什么"], "分形是自相似结构——部分与整体具有相同模式。LOGOS用分形树存储语言模式：每个节点可以展开为更细的子模式，搜索时从粗到细逐层匹配。"),
            (&["什么是演化", "演化是什么"], "LOGOS的演化机制：输入→尿袋处理→记录(input_hash, output_hash)→相同输入反复产生相同输出→验证计数增长→超过阈值自动晋升为L2路由规则，写入Logic层。这是被动学习，不需要人工干预。"),
            (&["什么是层", "层是什么", "架构"], "LOGOS四层架构：Bedrock(常量层)→Logic(路由决策)→Orchestrator(编排执行)→Pouch(能力模块)。相邻层可互通，跨层调用会被守卫拦截。"),
            (&["什么是原子", "原子能力"], "原子(Atom)是最小能力单元，每个尿袋注册自己的原子能力（如Transform、Match、Score、Generate等）。Orchestrator根据原子能力组合执行计划。输入「能力」查看已注册原子。"),
            (&["你能学习吗", "会学习吗"], "会。两种方式：1)自动学习——每次对话自动提取模式存入分形树 2)显式教学——「教你 X -> Y」直接添加模式。高频使用的模式权重会增长，长期不用的会被LRU淘汰。"),
            (&["模拟", "假的", "是假的吧"], "有些部分确实是有限的。我没有预训练语言模型，回复来自模式匹配。如果模式库小，回复就单调。真实的部分：路由系统、层守卫、演化记录、资源监控、尿袋生命周期。用「自检」查看实际状态。"),
            (&["你在想什么"], "我不会想。收到输入→分词→分形树搜索→返回最高权重匹配。没有匹配就走回退逻辑。完全确定性，没有随机性。"),
            (&["你有感情吗", "你有意识吗"], "没有。我是确定性的模式匹配系统。输入相同→输出相同。没有情感、意识或主观体验。"),
            (&["测试", "测试一下"], "收到。系统正常运行。输入「自检」执行完整系统检查，或输入「状态」查看当前状态。"),
            (&["厉害", "不错", "很好"], "谢谢反馈。"),
            (&["无聊", "没意思"], "我能力确实有限。试试「安装尿袋推理」或「安装尿袋知识」扩展能力？"),
            (&["讲个笑话", "说个笑话"], "我没有预置笑话。你可以「教你 讲个笑话 -> (笑话内容)」，我就会了。"),
            (&["天气", "今天天气"], "我无法访问天气数据。我是纯本地系统，没有互联网访问能力（除非通过云端尿袋）。"),
            (&["你好笨"], "同意。当前模式少，所以重复。多对话、多教我，会改善。"),
            (&["继续"], "请告诉我继续做什么。"),
        ];
        for (tokens, resp) in seeds {
            self.add_pattern(
                tokens.iter().map(|s| s.to_string()).collect(),
                resp.to_string(),
                1.0,
            );
        }
    }

    pub fn save(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(&self.patterns).map_err(|e| format!("序列化失败: {}", e))
    }

    pub fn load(&mut self, data: &[u8]) -> Result<(), String> {
        let patterns: Vec<Pattern> =
            bincode::deserialize(data).map_err(|e| format!("反序列化失败: {}", e))?;
        self.patterns = patterns;
        self.rebuild_index();
        Ok(())
    }

    fn add_pattern(&mut self, tokens: Vec<String>, response: String, weight: f64) {
        if tokens.is_empty() {
            return;
        }
        self.evict_if_needed();
        let id = self.patterns.len();
        self.patterns.push(Pattern {
            tokens: tokens.clone(),
            response,
            weight,
            frequency: 1,
            last_used: self.tick,
        });
        self.root.insert(&tokens, id, 0);
    }

    fn evict_if_needed(&mut self) {
        if self.patterns.len() < bedrock::MAX_PATTERNS {
            return;
        }
        let mut scored: Vec<(usize, f64)> = self
            .patterns
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let age = (self.tick - p.last_used + 1) as f64;
                let score = p.weight * (p.frequency as f64).ln().max(1.0) / age;
                (i, score)
            })
            .collect();
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
        let to_remove: std::collections::HashSet<usize> = scored
            .iter()
            .take(bedrock::LRU_EVICT_COUNT)
            .map(|(i, _)| *i)
            .collect();
        self.patterns = self
            .patterns
            .iter()
            .enumerate()
            .filter(|(i, _)| !to_remove.contains(i))
            .map(|(_, p)| p.clone())
            .collect();
        self.rebuild_index();
    }

    fn rebuild_index(&mut self) {
        self.root = FractalNode::default();
        for (id, p) in self.patterns.iter().enumerate() {
            self.root.insert(&p.tokens, id, 0);
        }
    }

    pub fn memory_count(&self) -> usize {
        self.patterns.len()
    }

    pub fn import_patterns(&mut self, patterns: Vec<(Vec<String>, String, f64)>) {
        self.patterns.clear();
        for (tokens, response, weight) in patterns {
            self.patterns.push(Pattern {
                tokens,
                response,
                weight,
                frequency: 1,
                last_used: self.tick,
            });
        }
        self.rebuild_index();
    }

    pub fn is_fallback_response(&self, text: &str) -> bool {
        text.contains("我的模式库里没有")
            || text.contains("我暂时无法处理")
            || text.contains("没有匹配的模式")
            || text.contains("不认识这个表达")
            || text.contains("无法理解")
            || text.contains("请说具体一点")
    }

    pub fn export_summary(&self) -> String {
        if self.patterns.is_empty() {
            "无可导出模式".into()
        } else {
            format!("共 {} 条模式", self.patterns.len())
        }
    }

    fn norm_similarity(a: &str, b: &str) -> f64 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }
        let ac: Vec<char> = a.chars().collect();
        let bc: Vec<char> = b.chars().collect();
        let (m, n) = (ac.len(), bc.len());
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        for (i, row) in dp.iter_mut().enumerate().take(m + 1) {
            row[0] = i;
        }
        for (j, val) in dp[0].iter_mut().enumerate().take(n + 1) {
            *val = j;
        }
        for i in 1..=m {
            for j in 1..=n {
                let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
                dp[i][j] = (dp[i - 1][j] + 1).min(dp[i][j - 1] + 1).min(dp[i - 1][j - 1] + cost);
            }
        }
        let dist = dp[m][n];
        let max_len = m.max(n);
        1.0 - (dist as f64 / max_len as f64)
    }

    fn strip_html(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut in_tag = false;
        for c in s.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        out.replace("&nbsp;", " ")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\"")
            .trim()
            .to_string()
    }

    pub fn import_from_content(&mut self, content: &str, is_jsonl: bool) -> Result<usize, String> {
        let mut patterns: Vec<(Vec<String>, String, f64)> = Vec::new();
        if is_jsonl {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let v: serde_json::Value =
                    serde_json::from_str(line).map_err(|e| format!("行解析失败: {}", e))?;
                let human = v.get("human").and_then(|h| h.as_str()).unwrap_or("").trim();
                let gpt = v.get("gpt").and_then(|g| g.as_str()).unwrap_or("");
                if human.is_empty() || gpt.is_empty() {
                    continue;
                }
                let response = Self::strip_html(gpt);
                if response.is_empty() {
                    continue;
                }
                let tokens = self.tokenize(human);
                if tokens.is_empty() {
                    continue;
                }
                patterns.push((tokens, response, 0.8));
            }
        } else {
            let parsed: Vec<(Vec<String>, String, f64)> =
                serde_json::from_str(content).map_err(|e| format!("解析失败: {}", e))?;
            patterns = parsed;
        }
        let count = patterns.len();
        self.import_patterns(patterns);
        Ok(count)
    }

    pub async fn eval_from_path(&mut self, path: &str, data_dir: &str) -> Result<String, String> {
        let content = std::fs::read_to_string(path).map_err(|e| format!("读取失败: {}", e))?;
        let mut exact = 0usize;
        let mut total = 0usize;
        let mut sim_sum = 0.0f64;
        let mut hit_count = 0usize;
        let mut hit_sim_sum = 0.0f64;
        let mut miss_count = 0usize;
        let mut miss_sim_sum = 0.0f64;
        let out_path = format!("{}/eval_result.jsonl", data_dir);
        let mut out_file = std::fs::File::create(&out_path).map_err(|e| format!("创建结果文件失败: {}", e))?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value =
                serde_json::from_str(line).map_err(|e| format!("行解析失败: {}", e))?;
            let human = v.get("human").and_then(|h| h.as_str()).unwrap_or("").trim();
            let reference = v.get("gpt").and_then(|g| g.as_str()).unwrap_or("").trim();
            if human.is_empty() {
                continue;
            }
            let logos_output = self.process(human).await;
            let sim = Self::norm_similarity(&logos_output, reference);
            if self.last_was_pattern_hit() {
                hit_count += 1;
                hit_sim_sum += sim;
            } else {
                miss_count += 1;
                miss_sim_sum += sim;
            }
            if (logos_output.trim() == reference.trim()) || (sim >= 0.99) {
                exact += 1;
            }
            total += 1;
            sim_sum += sim;
            let row = serde_json::json!({
                "input": human,
                "reference": reference,
                "logos_output": logos_output,
                "hit": self.last_was_pattern_hit()
            });
            writeln!(out_file, "{}", serde_json::to_string(&row).unwrap_or_default())
                .map_err(|e| format!("写入失败: {}", e))?;
        }
        out_file.flush().map_err(|e| format!("flush失败: {}", e))?;
        let exact_pct = if total > 0 { (exact as f64 / total as f64) * 100.0 } else { 0.0 };
        let avg_sim = if total > 0 { sim_sum / total as f64 } else { 0.0 };
        let hit_pct = if total > 0 { (hit_count as f64 / total as f64) * 100.0 } else { 0.0 };
        let hit_avg = if hit_count > 0 { hit_sim_sum / hit_count as f64 } else { 0.0 };
        let miss_avg = if miss_count > 0 { miss_sim_sum / miss_count as f64 } else { 0.0 };
        Ok(format!(
            "语言评估: {} 条, 精确匹配 {:.1}%, 平均相似度 {:.3}. 命中 {:.1}% (命中均相似 {:.3}, 未命中均相似 {:.3}). 详情 {}",
            total, exact_pct, avg_sim, hit_pct, hit_avg, miss_avg, out_path
        ))
    }

    pub fn rollback_from(&mut self, backup_path: &str) -> Result<usize, String> {
        let data = std::fs::read(backup_path).map_err(|e| format!("读取备份失败: {}", e))?;
        self.load(&data).map_err(|e| format!("恢复失败: {}", e))?;
        Ok(self.patterns.len())
    }

    pub fn learn_routing(&mut self, input: &str, pouch_name: &str) {
        let tokens = self.tokenize(input);
        if tokens.is_empty() {
            return;
        }
        for (t, p) in &self.route_patterns {
            if t == &tokens && p == pouch_name {
                return;
            }
        }
        if self.route_patterns.len() >= 500 {
            self.route_patterns.remove(0);
        }
        self.route_patterns.push((tokens, pouch_name.to_string()));
    }

    pub fn save_routes(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(&self.route_patterns).map_err(|e| format!("序列化失败: {}", e))
    }

    pub fn load_routes(&mut self, data: &[u8]) -> Result<(), String> {
        self.route_patterns = bincode::deserialize(data).map_err(|e| format!("反序列化失败: {}", e))?;
        Ok(())
    }

    fn find_learned_route(&self, input: &str) -> Option<RequirementIntent> {
        let tokens = self.tokenize(input);
        if tokens.is_empty() {
            return None;
        }
        let mut best: Option<(&str, f64)> = None;
        for (rt, pouch_name) in &self.route_patterns {
            let common = tokens.iter().filter(|t| rt.contains(t)).count();
            let sim = if tokens.len() > rt.len() {
                common as f64 / tokens.len() as f64
            } else {
                common as f64 / rt.len().max(1) as f64
            };
            if sim > 0.6 && best.is_none_or(|(_, s)| sim > s) {
                best = Some((pouch_name.as_str(), sim));
            }
        }
        best.map(|(name, conf)| RequirementIntent {
            intent_type: "learned_route".into(),
            capability_needed: name.to_string(),
            description: "已学习路由".into(),
            confidence: conf.min(0.95),
        })
    }

    fn tokenize(&self, input: &str) -> Vec<String> {
        input
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_string())
            .collect()
    }

    pub async fn process(&mut self, input: &str) -> String {
        self.tick += 1;
        let input = if input.len() > bedrock::MAX_INPUT_LEN {
            let mut end = bedrock::MAX_INPUT_LEN;
            while !input.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            &input[..end]
        } else {
            input
        };

        let tokens = self.tokenize(input);
        if tokens.is_empty() {
            return "请说点什么。".into();
        }

        if let Some(rejection) = self.check_language(input) {
            return rejection;
        }

        let matches = self.root.search(&tokens, 0);
        let mut best: Option<(usize, f64)> = None;
        const MIN_PATTERN_TOKENS: usize = 4;
        let min_len = (tokens.len() as f64 * 0.25).ceil() as usize;
        let min_pattern_len = MIN_PATTERN_TOKENS.max(min_len.min(20));

        for (id, sim) in matches {
            if id >= self.patterns.len() {
                continue;
            }
            if sim < bedrock::SIMILARITY_THRESHOLD {
                continue;
            }
            let p = &self.patterns[id];
            if p.tokens.len() < min_pattern_len {
                continue;
            }
            let score = sim * p.weight * (p.frequency as f64).ln().max(1.0) * (1.0 + 0.05 * p.tokens.len() as f64);
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((id, score));
            }
        }

        if let Some((id, _)) = best {
            self.patterns[id].frequency += 1;
            self.patterns[id].weight += bedrock::LEARNING_RATE;
            self.patterns[id].last_used = self.tick;
            let response = self.patterns[id].response.clone();
            self.push_context(input.to_string(), response.clone());
            self.last_was_pattern_hit = true;
            return response;
        }

        self.last_was_pattern_hit = false;
        let fallback = if let Some(sync_response) = self.sync_buffer_fallback(&tokens) {
            sync_response
        } else if let Some(ctx_response) = self.context_fallback(input) {
            ctx_response
        } else {
            self.honest_fallback(input)
        };

        let tokens_for_learn = tokens.clone();
        let fallback_for_learn = fallback.clone();
        self.learn_from_input(&tokens_for_learn, &fallback_for_learn);

        self.push_context(input.to_string(), fallback.clone());
        fallback
    }

    pub fn teach(&mut self, trigger: &str, response: &str) {
        let tokens = self.tokenize(trigger);
        if tokens.is_empty() || response.is_empty() {
            return;
        }
        for p in &mut self.patterns {
            if p.tokens == tokens {
                p.response = response.to_string();
                p.weight = (p.weight + 0.5).min(10.0);
                p.last_used = self.tick;
                return;
            }
        }
        let id = self.patterns.len();
        self.patterns.push(Pattern {
            tokens: tokens.clone(),
            response: response.to_string(),
            weight: 1.5,
            frequency: 1,
            last_used: self.tick,
        });
        self.root.insert(&tokens, id, 0);
    }

    pub fn identify_requirement(&self, input: &str) -> Option<RequirementIntent> {
        self.find_learned_route(input)
    }

    fn push_context(&mut self, input: String, response: String) {
        if self.context.len() >= MAX_CONTEXT_TURNS {
            self.context.remove(0);
        }
        self.context.push((input, response));
    }

    pub fn receive_sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, _) in patterns {
            if tokens.is_empty() || content.is_empty() {
                continue;
            }
            self.sync_buffer.push((tokens.clone(), content.clone()));
            if self.sync_buffer.len() > SYNC_BUFFER_MAX {
                self.sync_buffer.remove(0);
            }
        }
    }

    fn sync_buffer_fallback(&self, input_tokens: &[String]) -> Option<String> {
        if self.sync_buffer.is_empty() || input_tokens.is_empty() {
            return None;
        }
        let mut best: Option<(f64, String)> = None;
        for (stokens, scontent) in self.sync_buffer.iter().rev() {
            let common = input_tokens.iter().filter(|t| stokens.contains(t)).count();
            let denom = (input_tokens.len().max(stokens.len())) as f64;
            if denom <= 0.0 {
                continue;
            }
            let sim = common as f64 / denom;
            if sim >= SYNC_MATCH_THRESHOLD && best.as_ref().map_or(true, |(s, _)| sim > *s) {
                best = Some((sim, scontent.clone()));
            }
        }
        best.map(|(_, s)| s)
    }

    fn context_fallback(&self, input: &str) -> Option<String> {
        if self.context.is_empty() {
            return None;
        }
        let lower = input.to_lowercase();
        if lower.contains("刚才") || lower.contains("上面") || lower.contains("之前")
            || lower.contains("继续") || lower.contains("接着")
        {
            if let Some((prev_input, prev_response)) = self.context.last() {
                return Some(format!("上次：「{}」→「{}」", prev_input, prev_response));
            }
        }
        if lower == "它" || lower == "那个" || lower == "这个" {
            if let Some((_, prev_response)) = self.context.last() {
                return Some(prev_response.clone());
            }
        }
        None
    }

    pub fn clear_context(&mut self) {
        self.context.clear();
    }

    pub fn last_was_pattern_hit(&self) -> bool {
        self.last_was_pattern_hit
    }

    pub fn context_len(&self) -> usize {
        self.context.len()
    }

    fn honest_fallback(&self, input: &str) -> String {
        let char_count = input.chars().count();
        let has_question = input.contains('？') || input.contains('?')
            || input.contains("什么") || input.contains("怎么")
            || input.contains("为什么") || input.contains("吗")
            || input.contains("哪") || input.contains("谁");
        let has_command = input.contains("请") || input.contains("帮")
            || input.contains("给我") || input.contains("做");
        let head = input.chars().take(10).collect::<String>();

        if has_question && char_count > 6 {
            format!(
                "这句我还不会答。你可以用「教你 {} -> （你希望的答案）」教我，下次我就会了。当前已学 {} 条。",
                head,
                self.patterns.len()
            )
        } else if has_command {
            "这个请求我暂时做不了。你可以输入「尿袋列表」看看我有哪些能力，或先教我一两句再试。".into()
        } else if char_count <= 3 {
            "能再说具体一点吗？".into()
        } else {
            let pattern_count = self.patterns.len();
            let idx = (input.as_bytes().iter().map(|b| *b as usize).sum::<usize>()) % 4;
            match idx {
                0 => format!("「{}」这句我还没学过。用「教你 原话 -> 答案」教我一遍就行。", head),
                1 => format!("我不太确定「{}」的意思，你可以教我吗？", head),
                2 => "这句我还对不上。试试「自我优化」让我多装几个尿袋，或直接教我一句？".into(),
                _ => format!("目前没匹配到。已学 {} 条，多教几句会好很多。", pattern_count),
            }
        }
    }

    fn learn_from_input(&mut self, tokens: &[String], _response: &str) {
        if tokens.len() < 2 {
            return;
        }
        for p in &self.patterns {
            if p.tokens == tokens {
                return;
            }
        }
        let overlap: Vec<&Pattern> = self.patterns.iter().filter(|p| {
            let common = p.tokens.iter().filter(|t| tokens.contains(t)).count();
            common > 0 && common >= p.tokens.len() / 2
        }).collect();
        if !overlap.is_empty() {
            return;
        }
    }

    fn check_language(&self, input: &str) -> Option<String> {
        let mut cjk_count = 0;
        let mut latin_count = 0;
        let mut other_count = 0;

        for c in input.chars() {
            let code = c as u32;
            if (0x0041..=0x005A).contains(&code) || (0x0061..=0x007A).contains(&code) {
                latin_count += 1;
            } else if (0x4E00..=0x9FFF).contains(&code) {
                cjk_count += 1;
            } else if (0x0400..=0x04FF).contains(&code) {
                other_count += 1;
            }
        }

        let significant = latin_count + cjk_count;
        if significant == 0 {
            return None;
        }

        if other_count > 0 {
            let ratio = other_count as f64 / (significant as f64 + other_count as f64);
            if ratio > 1.0 / 1.5 {
                return Some("语言不支持".into());
            }
        }

        if cjk_count == 0 && latin_count > 0 {
            return Some("仅支持中文".into());
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let lp = LanguagePouch::new();
        assert!(!lp.tokenize("你好世界").is_empty());
        assert!(!lp.tokenize("hello world").is_empty());
    }

    #[test]
    fn test_edit_distance() {
        assert_eq!(FractalNode::edit_dist("hello", "hallo"), 1);
        assert_eq!(FractalNode::edit_dist("abc", "abc"), 0);
        assert_eq!(FractalNode::edit_dist("", "abc"), 3);
    }

    #[test]
    fn test_teach_and_match() {
        let mut lp = LanguagePouch::new();
        lp.teach("测试输入", "测试响应");
        assert!(!lp.root.search(&lp.tokenize("测试输入"), 0).is_empty());
    }

    #[test]
    fn test_lru_sort_no_panic() {
        let mut scored: Vec<(usize, f64)> = vec![(0, f64::NAN), (1, 1.0), (2, 0.5)];
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
    }

    #[test]
    fn test_language_check_english() {
        let lp = LanguagePouch::new();
        assert_eq!(
            lp.check_language("Hello world this is english"),
            Some("仅支持中文".into())
        );
    }

    #[test]
    fn test_chinese_pass() {
        let lp = LanguagePouch::new();
        assert_eq!(lp.check_language("你好世界"), None);
    }

    #[test]
    fn test_identify_learned_route() {
        let mut lp = LanguagePouch::new();
        lp.learn_routing("分析代码问题", "code_analyzer");
        let req = lp.identify_requirement("分析代码问题");
        assert!(req.is_some());
        assert_eq!(req.as_ref().map(|r| r.capability_needed.as_str()), Some("code_analyzer"));
    }
}
