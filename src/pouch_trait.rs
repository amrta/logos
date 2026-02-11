use std::time::{SystemTime, UNIX_EPOCH};
use std::path::PathBuf;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use async_trait::async_trait;
use crate::atom::{AtomKind, AtomDeclaration};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PouchRole { E0, E1, E2 }

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Debug, Clone)]
pub struct PouchOutput {
    pub data: String,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct ProposalMessage {
    pub proposal_type: String,
    pub content: String,
    pub confidence: f32,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ValidatedProposal {
    inner: ProposalMessage,
}

impl ValidatedProposal {
    pub fn inner(&self) -> &ProposalMessage {
        &self.inner
    }
}

#[derive(Debug, Clone)]
pub struct ProposalValidator {
    pub allowed_types: Vec<String>,
    pub min_confidence: f32,
    pub min_evidence_count: usize,
}

impl ProposalValidator {
    pub fn validate(&self, proposal: &ProposalMessage) -> Result<ValidatedProposal, String> {
        if !self.allowed_types.contains(&proposal.proposal_type) {
            return Err(format!("type_rejected: '{}'", proposal.proposal_type));
        }
        if proposal.confidence < self.min_confidence {
            return Err(format!("confidence_low: {} < {}", proposal.confidence, self.min_confidence));
        }
        if proposal.evidence.len() < self.min_evidence_count {
            return Err(format!("evidence_insufficient: {} < {}", proposal.evidence.len(), self.min_evidence_count));
        }
        Ok(ValidatedProposal {
            inner: proposal.clone(),
        })
    }
}

#[async_trait]
pub trait Pouch: Send + Sync {
    fn name(&self) -> &str;
    fn role(&self) -> PouchRole;
    fn validator(&self) -> &ProposalValidator;
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String>;
    fn can_call_others(&self) -> bool {
        matches!(self.role(), PouchRole::E1 | PouchRole::E2)
    }
    fn sync_patterns(&mut self, _patterns: &[(Vec<String>, String, f64)]) {
    }
    fn memory_count(&self) -> usize;
    fn explain(&self) -> String;
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![]
    }
    fn is_fallback_output(&self, _output: &str) -> bool {
        false
    }
    fn recommended_follow_ups(&self, _my_output: &str) -> Vec<String> {
        vec![]
    }
    fn evolution_gaps_from_output(&self, _my_output: &str) -> Vec<(String, f64)> {
        vec![]
    }
}

#[derive(Debug, Clone)]
pub struct PouchMeta {
    pub role: PouchRole,
}

pub struct MaterialPouch {
    name: String,
    validator: ProposalValidator,
    elements: Vec<String>,
}

impl MaterialPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["material_analysis".into(), "pipeline_data".into()],
                min_confidence: 0.4,
                min_evidence_count: 0,
            },
            elements: vec!["Fe".into(), "C".into(), "Al".into(), "Cu".into(), "Si".into()],
        }
    }

    fn analyze(&self, input: &str) -> String {
        let mut found = Vec::new();
        for elem in &self.elements {
            if input.contains(elem.as_str()) {
                found.push(elem.clone());
            }
        }
        if found.is_empty() {
            format!("MaterialPouch[{}]: 未检测到已知元素", self.name)
        } else {
            format!("MaterialPouch[{}]: 检测到元素 {:?}", self.name, found)
        }
    }
}

#[async_trait]
impl Pouch for MaterialPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let analysis = self.analyze(&proposal.inner().content);
        Ok(PouchOutput { data: analysis, confidence: 0.85 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String {
        format!("MaterialPouch: 材料分析尿袋，支持{}种元素", self.elements.len())
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "material_analyze".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.95),
        }]
    }
}

pub struct PrinterPouch {
    name: String,
    validator: ProposalValidator,
}

impl PrinterPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["print_request".into(), "gcode_generate".into(), "pipeline_data".into()],
                min_confidence: 0.3,
                min_evidence_count: 0,
            },
        }
    }

    fn generate_gcode(&self, input: &str) -> String {
        let has_material = input.contains("material") || input.contains("材料") || input.contains("密度") || input.contains("MaterialPouch");
        if has_material {
            let mut end = input.len().min(50);
            while !input.is_char_boundary(end) && end > 0 { end -= 1; }
            format!("G-Code生成:\nG28\nG1 Z5 F3000\nM104 S200\n基于材料数据: {}", &input[..end])
        } else {
            "G-Code: 需要材料数据输入。请先通过材料尿袋分析。".into()
        }
    }
}

#[async_trait]
impl Pouch for PrinterPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E2 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let gcode = self.generate_gcode(&proposal.inner().content);
        Ok(PouchOutput { data: gcode, confidence: 0.9 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "PrinterPouch: 3D打印G-Code生成尿袋".into() }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "gcode_generate".into(),
            kind: AtomKind::Generate,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.95),
        }]
    }
}

pub fn create_proposal(content: &str) -> ProposalMessage {
    ProposalMessage {
        proposal_type: "pipeline_data".into(),
        content: content.to_string(),
        confidence: 0.8,
        evidence: vec![],
    }
}

pub struct ReasoningPouch {
    name: String,
    validator: ProposalValidator,
}

impl ReasoningPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["reasoning_query".into(), "math_query".into(), "logic_query".into(), "pipeline_data".into()],
                min_confidence: 0.2,
                min_evidence_count: 0,
            },
        }
    }

    fn process_reasoning(&self, input: &str) -> String {
        let lower = input.to_lowercase();
        if let Some(result) = self.try_math(&lower) {
            return format!("推理结果: {}", result);
        }
        if lower.contains("如果") && (lower.contains("那么") || lower.contains("则")) {
            return self.logical_inference(input);
        }
        if (lower.contains("比") && lower.contains("大")) ||
           (lower.contains("比") && lower.contains("小")) ||
           lower.contains("最大") || lower.contains("最小") {
            return self.comparison_reasoning(input);
        }
        "推理尿袋: 请提供数学表达式、逻辑判断或比较关系。".into()
    }

    fn try_math(&self, input: &str) -> Option<String> {
        let input = input.replace("加", "+").replace("减", "-")
                        .replace("乘", "*").replace("除", "/")
                        .replace(['？', '?'], "");
        let chars: Vec<char> = input.chars().filter(|c| c.is_numeric() || "+-*/. ".contains(*c)).collect();
        let expr: String = chars.into_iter().collect();
        let expr = expr.trim();
        if expr.is_empty() { return None; }
        if let Some(result) = self.eval_simple(expr) {
            return Some(format!("{} = {}", expr, result));
        }
        None
    }

    fn eval_simple(&self, expr: &str) -> Option<f64> {
        for op in ['+', '-', '*', '/'] {
            if let Some(pos) = expr.find(op) {
                let left = expr[..pos].trim().parse::<f64>().ok()?;
                let right = expr[pos+1..].trim().parse::<f64>().ok()?;
                return Some(match op {
                    '+' => left + right,
                    '-' => left - right,
                    '*' => left * right,
                    '/' => if right != 0.0 { left / right } else { return None; },
                    _ => return None,
                });
            }
        }
        None
    }

    fn logical_inference(&self, input: &str) -> String {
        let mut end = input.len().min(30);
        while !input.is_char_boundary(end) && end > 0 { end -= 1; }
        format!("逻辑推理: 根据「{}」的条件，建议先明确前提条件再推导结论。", &input[..end])
    }

    fn comparison_reasoning(&self, input: &str) -> String {
        if input.contains('A') && input.contains('B') && input.contains('C')
            && input.contains("比") && input.contains("大")
        {
            return "传递性推理: 如果 A>B 且 B>C，则 A>C，所以 A 最大".into();
        }
        "比较推理: 检测到比较关系，建议明确具体对象和关系".into()
    }
}

#[async_trait]
impl Pouch for ReasoningPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let result = self.process_reasoning(&proposal.inner().content);
        Ok(PouchOutput { data: result, confidence: 0.7 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "ReasoningPouch: 推理尿袋，处理逻辑推理、数学计算、比较分析".into() }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![
            AtomDeclaration {
                name: "math_eval".into(),
                kind: AtomKind::Score,
                pouch: self.name.clone(),
                confidence_range: (0.5, 0.95),
            },
            AtomDeclaration {
                name: "logical_infer".into(),
                kind: AtomKind::Match,
                pouch: self.name.clone(),
                confidence_range: (0.4, 0.85),
            },
            AtomDeclaration {
                name: "comparison".into(),
                kind: AtomKind::Transform,
                pouch: self.name.clone(),
                confidence_range: (0.5, 0.9),
            },
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub tokens: Vec<String>,
    pub content: String,
    pub weight: f64,
    pub theme: String,
    pub timestamp: u64,
}

pub struct MemoryPouch {
    name: String,
    validator: ProposalValidator,
    memories: Vec<MemoryItem>,
    save_path: PathBuf,
}

impl MemoryPouch {
    pub fn new(name: &str, data_dir: &str) -> Result<Self, String> {
        let save_path = PathBuf::from(data_dir)
            .join("memories")
            .join(format!("{}.bin", name));
        let mut pouch = Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["memory_query".into(), "store_memory".into(), "pipeline_data".into()],
                min_confidence: 0.1,
                min_evidence_count: 0,
            },
            memories: Vec::new(),
            save_path,
        };
        pouch.load()?;
        Ok(pouch)
    }

    fn load(&mut self) -> Result<(), String> {
        if !self.save_path.exists() {
            return Ok(());
        }
        let data = std::fs::read(&self.save_path)
            .map_err(|e| format!("读文件失败: {}", e))?;
        self.memories = bincode::deserialize(&data)
            .map_err(|e| format!("反序列化失败: {}", e))?;
        log::info!("MemoryPouch({}) 加载成功: {} 条记忆", self.name, self.memories.len());
        Ok(())
    }

    fn store(&mut self, tokens: Vec<String>, content: String, weight: f64, theme: &str) -> Result<(), String> {
        if content.len() < 5 {
            return Ok(());
        }
        if self.memories.len() >= 2000 {
            self.memories.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
            self.memories.truncate(1500);
        }
        self.memories.push(MemoryItem {
            tokens,
            content,
            weight,
            theme: theme.to_string(),
            timestamp: current_timestamp(),
        });
        if let Some(parent) = self.save_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建目录失败: {}", e))?;
        }
        let data = bincode::serialize(&self.memories)
            .map_err(|e| format!("序列化失败: {}", e))?;
        std::fs::write(&self.save_path, data)
            .map_err(|e| format!("写文件失败: {}", e))?;
        Ok(())
    }
}

#[async_trait]
impl Pouch for MemoryPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }

    async fn process_proposal(&mut self, _proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        if self.memories.is_empty() {
            return Ok(PouchOutput { data: "记忆库为空".into(), confidence: 0.5 });
        }
        Ok(PouchOutput {
            data: format!("记忆库状态：共 {} 条记忆", self.memories.len()),
            confidence: 0.8,
        })
    }

    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 1.0 {
                let theme = if content.contains("诗") || content.contains("词") {
                    "诗歌"
                } else if content.contains("故事") {
                    "故事"
                } else {
                    "对话"
                };
                if let Err(e) = self.store(tokens.clone(), content.clone(), *weight, theme) {
                    log::error!("MemoryPouch::sync_patterns 存储失败: {}", e);
                }
            }
        }
    }

    fn memory_count(&self) -> usize { self.memories.len() }
    fn explain(&self) -> String {
        format!("MemoryPouch: 记忆尿袋，存储{}条高质量记忆", self.memories.len())
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "memory_store".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.8, 0.95),
        }]
    }
}

pub struct CreativePouch {
    name: String,
    validator: ProposalValidator,
}

impl CreativePouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["creative_request".into(), "generation_request".into(), "pipeline_data".into()],
                min_confidence: 0.2,
                min_evidence_count: 0,
            },
        }
    }
}

#[async_trait]
impl Pouch for CreativePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, _proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        Err("E2尿袋需安装具体实现".to_string())
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { format!("CreativePouch: 创造尿袋 {}", self.name) }
}

pub struct CloudTrainerPouch {
    name: String,
    validator: ProposalValidator,
    pending_data: Vec<(String, String)>,
    trained: bool,
}

impl CloudTrainerPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["training_request".into(), "auto_train".into(), "collect_data".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            pending_data: Vec::new(),
            trained: false,
        }
    }

    async fn train_async(&mut self) -> Result<String, String> {
        if self.pending_data.is_empty() {
            return Err("没有训练数据".to_string());
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("创建客户端失败: {}", e))?;
        let pairs: Vec<serde_json::Value> = self.pending_data.iter()
            .map(|(input, output)| serde_json::json!({ "input": input, "output": output }))
            .collect();
        let endpoint = "https://logos-gateway.amrta.workers.dev/train";
        let response = client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "pairs": pairs }))
            .send()
            .await
            .map_err(|e| format!("云端训练请求失败: {}", e))?;
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_else(|_| "无法读取错误".to_string());
            return Err(format!("云端训练失败: {}", error_text));
        }
        let json: serde_json::Value = response.json().await
            .map_err(|e| format!("解析失败: {}", e))?;
        let count = json["patterns"].as_array().map(|a| a.len()).unwrap_or(0);
        self.trained = true;
        Ok(format!("云端训练完成，生成 {} 条模式", count))
    }
}

#[async_trait]
impl Pouch for CloudTrainerPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let inner = proposal.inner();
        if inner.proposal_type == "collect_data" {
            if let Some((input, output)) = inner.content.split_once("|||") {
                self.pending_data.push((input.to_string(), output.to_string()));
                return Ok(PouchOutput {
                    data: format!("已收集，当前 {} 条", self.pending_data.len()),
                    confidence: 1.0,
                });
            }
        }
        if inner.content == "train" || inner.proposal_type == "training_request" {
            return match self.train_async().await {
                Ok(msg) => Ok(PouchOutput { data: msg, confidence: 0.95 }),
                Err(e) => Err(e),
            };
        }
        Ok(PouchOutput {
            data: format!("CloudTrainer 待训练: {} 条", self.pending_data.len()),
            confidence: 0.9,
        })
    }
    fn memory_count(&self) -> usize { self.pending_data.len() }
    fn explain(&self) -> String {
        format!("CloudTrainerPouch: 云端训练尿袋，待训练 {} 条，状态: {}",
            self.pending_data.len(),
            if self.trained { "已完成" } else { "待执行" })
    }
}

pub struct DiscoveryPouch {
    name: String,
    validator: ProposalValidator,
}

impl DiscoveryPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["discovery".into(), "failover_scan".into(), "pipeline_data".into()],
                min_confidence: 0.1,
                min_evidence_count: 0,
            },
        }
    }

    async fn discover_services(&self) -> String {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(c) => c,
            Err(_) => return r#"{"error":"无法连接","services":[]}"#.to_string(),
        };
        let endpoint = "https://logos-gateway.amrta.workers.dev/discover";
        match client.get(endpoint).send().await {
            Ok(resp) => resp.text().await.unwrap_or_else(|_| r#"{"error":"无法读取响应"}"#.into()),
            Err(_) => r#"{"error":"发现服务不可用","services":[]}"#.to_string(),
        }
    }
}

#[async_trait]
impl Pouch for DiscoveryPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, _proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let result = self.discover_services().await;
        Ok(PouchOutput { data: result, confidence: 0.9 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "DiscoveryPouch: 扫描互联网可用的容灾服务供配置".into() }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "service_scan".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}

pub struct ContextAwarePouch {
    name: String,
    validator: ProposalValidator,
    terminology: HashMap<String, (String, String, Vec<String>)>,
}

impl ContextAwarePouch {
    pub fn new(name: &str) -> Self {
        let mut pouch = Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["context_analysis".into(), "term_disambiguation".into(), "pipeline_data".into()],
                min_confidence: 0.5,
                min_evidence_count: 0,
            },
            terminology: HashMap::new(),
        };
        pouch.init_terminology();
        pouch
    }

    fn init_terminology(&mut self) {
        let terms = vec![
            ("尾袋", ("LOGOS 的可插拔模块系统", "LOGOS", vec!["Pouch".into(), "模块".into()])),
            ("LanguagePouch", ("低熵语言路由层", "LOGOS", vec!["语言尾袋".into()])),
            ("Bedrock", ("LOGOS 的公式层", "LOGOS", vec!["公式层".into()])),
            ("Logic", ("LOGOS 的逻辑层", "LOGOS", vec!["逻辑层".into()])),
            ("Orchestrator", ("尾袋管理器", "LOGOS", vec!["管理器".into()])),
        ];
        for (term, (def, domain, similar)) in terms {
            self.terminology.insert(term.to_string(), (def.to_string(), domain.to_string(), similar));
        }
    }

    fn disambiguate(&self, term: &str) -> String {
        if let Some((def, domain, similar)) = self.terminology.get(term) {
            format!("【{}】(领域: {})\n定义：{}\n相关词：{}", term, domain, def, similar.join("、"))
        } else {
            format!("未知术语：{}", term)
        }
    }

    fn analyze_context(&self, input: &str) -> String {
        if input.contains("尾袋") || input.contains("Pouch") {
            return "【检测到 LOGOS 内部术语】".into();
        }
        "【通用语境】".into()
    }
}

#[async_trait]
impl Pouch for ContextAwarePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let inner = proposal.inner();
        let result = match inner.proposal_type.as_str() {
            "context_analysis" => self.analyze_context(&inner.content),
            "term_disambiguation" => self.disambiguate(&inner.content),
            _ => format!("未知的上下文分析类型: {}", inner.proposal_type),
        };
        Ok(PouchOutput { data: result, confidence: 0.85 })
    }
    fn memory_count(&self) -> usize { self.terminology.len() }
    fn explain(&self) -> String {
        format!("ContextAwarePouch: 术语消歧义，已加载 {} 术语", self.terminology.len())
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "term_disambiguate".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.9),
        }]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Molecule {
    pub formula: String,
    pub elements: HashMap<String, u32>,
    pub properties: HashMap<String, String>,
}

pub struct ChemistryPouch {
    name: String,
    validator: ProposalValidator,
    molecules: HashMap<String, Molecule>,
    periodic_table: HashMap<String, (String, f32)>,
}

impl ChemistryPouch {
    pub fn new(name: &str) -> Self {
        let mut pouch = Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["analyze_molecule".into(), "create_material".into(), "element_interaction".into(), "pipeline_data".into()],
                min_confidence: 0.5,
                min_evidence_count: 0,
            },
            molecules: HashMap::new(),
            periodic_table: HashMap::new(),
        };
        pouch.init_periodic_table();
        pouch.init_common_molecules();
        pouch
    }

    fn init_periodic_table(&mut self) {
        for (symbol, name, weight) in [
            ("H", "氢", 1.008), ("C", "碳", 12.011), ("N", "氮", 14.007),
            ("O", "氧", 15.999), ("Fe", "铁", 55.845), ("Cu", "铜", 63.546),
            ("Au", "金", 196.967), ("Si", "硅", 28.086), ("Al", "铝", 26.982),
        ] {
            self.periodic_table.insert(symbol.to_string(), (name.to_string(), weight));
        }
    }

    fn init_common_molecules(&mut self) {
        for (formula, name, props) in [
            ("H2O", "水", vec![("应用", "通用溶剂")]),
            ("CO2", "二氧化碳", vec![("应用", "灭火剂")]),
            ("NaCl", "氯化钠", vec![("用途", "食盐")]),
            ("Fe2O3", "三氧化二铁", vec![("用途", "铁矿")]),
            ("SiO2", "二氧化硅", vec![("用途", "玻璃")]),
        ] {
            let mut mol = Molecule {
                formula: formula.to_string(),
                elements: HashMap::new(),
                properties: HashMap::new(),
            };
            mol.properties.insert("名称".into(), name.to_string());
            for (k, v) in props {
                mol.properties.insert(k.to_string(), v.to_string());
            }
            self.parse_formula(formula, &mut mol);
            self.molecules.insert(formula.to_string(), mol);
        }
    }

    fn parse_formula(&self, formula: &str, mol: &mut Molecule) {
        let mut chars = formula.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch.is_uppercase() {
                let mut element = ch.to_string();
                while let Some(&next) = chars.peek() {
                    if next.is_lowercase() {
                        if let Some(c) = chars.next() { element.push(c); }
                    } else {
                        break;
                    }
                }
                let mut count_str = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_numeric() {
                        if let Some(c) = chars.next() { count_str.push(c); }
                    } else {
                        break;
                    }
                }
                let count: u32 = if count_str.is_empty() { 1 } else { count_str.parse().unwrap_or(1) };
                *mol.elements.entry(element).or_insert(0) += count;
            }
        }
    }

    fn calculate_molar_mass(&self, mol: &Molecule) -> f32 {
        mol.elements.iter()
            .map(|(elem, count)| {
                let mass = self.periodic_table.get(elem).map(|(_, m)| m).unwrap_or(&0.0);
                mass * (*count as f32)
            })
            .sum()
    }

    fn analyze_molecule(&self, formula: &str) -> String {
        if let Some(mol) = self.molecules.get(formula) {
            let molar_mass = self.calculate_molar_mass(mol);
            let props_str = mol.properties.iter()
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("【分子分析】\n分子式: {}\n相对分子质量: {:.2}\n属性: {}", formula, molar_mass, props_str)
        } else {
            format!("【新分子】\n分子式: {} (未知)", formula)
        }
    }

    fn create_material(&mut self, formula: &str, properties: Vec<(String, String)>) -> String {
        let mut mol = Molecule {
            formula: formula.to_string(),
            elements: HashMap::new(),
            properties: HashMap::new(),
        };
        self.parse_formula(formula, &mut mol);
        for (key, val) in properties {
            mol.properties.insert(key, val);
        }
        let molar_mass = self.calculate_molar_mass(&mol);
        mol.properties.insert("相对分子质量".into(), format!("{:.2}", molar_mass));
        self.molecules.insert(formula.to_string(), mol);
        format!("新材料已创建: {}", formula)
    }

    fn analyze_element_interaction(&self, elem1: &str, elem2: &str) -> String {
        let name1 = self.periodic_table.get(elem1).map(|(n, _)| n.as_str()).unwrap_or("未知");
        let name2 = self.periodic_table.get(elem2).map(|(n, _)| n.as_str()).unwrap_or("未知");
        let interaction = match (elem1, elem2) {
            ("H", "O") | ("O", "H") => "极强，形成 H-O 键",
            ("C", "H") | ("H", "C") => "强共价键，形成烃类",
            ("Fe", "O") | ("O", "Fe") => "强离子-共价键，铁氧化物",
            ("Si", "O") | ("O", "Si") => "强共价键，硅酸盐",
            _ => "相互作用类型需要更多信息",
        };
        format!("【元素相互作用】\n{} + {} → {}", name1, name2, interaction)
    }
}

#[async_trait]
impl Pouch for ChemistryPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let inner = proposal.inner();
        let result = match inner.proposal_type.as_str() {
            "analyze_molecule" => self.analyze_molecule(&inner.content),
            "create_material" => {
                let parts: Vec<&str> = inner.content.split('|').collect();
                if parts.is_empty() {
                    "错误：缺少分子式".into()
                } else {
                    let formula = parts[0];
                    let mut props = Vec::new();
                    for part in parts.iter().skip(1) {
                        if let Some(idx) = part.find(':') {
                            props.push((part[..idx].trim().to_string(), part[idx+1..].trim().to_string()));
                        }
                    }
                    self.create_material(formula, props)
                }
            }
            "element_interaction" => {
                let parts: Vec<&str> = inner.content.split('-').collect();
                if parts.len() >= 2 {
                    self.analyze_element_interaction(parts[0].trim(), parts[1].trim())
                } else {
                    "错误：格式应为 'H-O'".into()
                }
            }
            _ => format!("未知类型: {}", inner.proposal_type),
        };
        Ok(PouchOutput { data: result, confidence: 0.88 })
    }
    fn memory_count(&self) -> usize { self.molecules.len() }
    fn explain(&self) -> String {
        format!("ChemistryPouch: 分子分析，已加载 {} 分子，{} 元素",
            self.molecules.len(), self.periodic_table.len())
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![
            AtomDeclaration {
                name: "molecule_analyze".into(),
                kind: AtomKind::Transform,
                pouch: self.name.clone(),
                confidence_range: (0.7, 0.95),
            },
            AtomDeclaration {
                name: "material_create".into(),
                kind: AtomKind::Generate,
                pouch: self.name.clone(),
                confidence_range: (0.6, 0.9),
            },
            AtomDeclaration {
                name: "element_interact".into(),
                kind: AtomKind::Match,
                pouch: self.name.clone(),
                confidence_range: (0.5, 0.85),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_material_pouch() {
        let mut mp = MaterialPouch::new("material");
        let proposal = create_proposal("分析Fe和Al合金");
        let validated = match mp.validator().validate(&proposal) {
            Ok(v) => v,
            Err(e) => panic!("validation: {}", e),
        };
        let result = match mp.process_proposal(&validated).await {
            Ok(r) => r,
            Err(e) => panic!("process: {}", e),
        };
        assert!(result.data.contains("Fe"));
        assert!(result.data.contains("Al"));
    }

    #[tokio::test]
    async fn test_printer_pouch() {
        let mut pp = PrinterPouch::new("printer");
        let proposal = create_proposal("MaterialPouch: Fe密度7.8");
        let validated = match pp.validator().validate(&proposal) {
            Ok(v) => v,
            Err(e) => panic!("validation: {}", e),
        };
        let result = match pp.process_proposal(&validated).await {
            Ok(r) => r,
            Err(e) => panic!("process: {}", e),
        };
        assert!(result.data.contains("G28"));
    }

    #[test]
    fn test_pouch_roles() {
        let mp = MaterialPouch::new("m");
        let pp = PrinterPouch::new("p");
        assert_eq!(mp.role(), PouchRole::E1);
        assert_eq!(pp.role(), PouchRole::E2);
        assert!(mp.can_call_others());
        assert!(pp.can_call_others());
    }

    #[test]
    fn test_validator() {
        let validator = ProposalValidator {
            allowed_types: vec!["test".into()],
            min_confidence: 0.5,
            min_evidence_count: 1,
        };
        let proposal = ProposalMessage {
            proposal_type: "test".into(),
            content: "hello".into(),
            confidence: 0.6,
            evidence: vec!["e1".into()],
        };
        assert!(validator.validate(&proposal).is_ok());
        let bad = ProposalMessage {
            proposal_type: "invalid".into(),
            content: "hello".into(),
            confidence: 0.6,
            evidence: vec!["e1".into()],
        };
        assert!(validator.validate(&bad).is_err());
    }
}
