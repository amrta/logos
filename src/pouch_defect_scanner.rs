use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct DefectScannerPouch {
    name: String,
    validator: ProposalValidator,
    learned: Vec<(Vec<String>, String)>,
}

impl DefectScannerPouch {
    pub fn new() -> Self {
        Self {
            name: "defect_scanner".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["scan".to_string(), "pipeline_data".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            learned: Vec::new(),
        }
    }

    pub fn scan_real(&self, input: &str) -> String {
        let metrics: SystemState = if let Ok(m) = serde_json::from_str(input) {
            m
        } else {
            return "缺陷扫描需要系统状态数据（JSON格式）".to_string();
        };

        let mut defects: Vec<(String, u8, String)> = Vec::new();

        if metrics.pattern_count < 50 {
            defects.push(("D-PAT".into(), 9, format!("语言模式严重不足: {}条（建议>500）", metrics.pattern_count)));
        } else if metrics.pattern_count < 500 {
            defects.push(("D-PAT".into(), 6, format!("语言模式偏少: {}条（建议>500）", metrics.pattern_count)));
        }

        let core_pouches = ["benchmark", "defect_scanner", "capability_comparer", "reasoning", "knowledge_retriever", "code_analyzer", "memory"];
        let mut missing: Vec<&str> = Vec::new();
        for name in &core_pouches {
            if !metrics.installed_pouches.iter().any(|p| p.contains(name)) {
                missing.push(name);
            }
        }
        if !missing.is_empty() {
            let severity = if missing.len() > 4 { 8 } else { 5 };
            defects.push(("D-PCH".into(), severity, format!("缺少核心尿袋: {}", missing.join(", "))));
        }

        let sleeping: Vec<&String> = metrics.installed_pouches.iter()
            .zip(metrics.pouch_awake.iter())
            .filter(|(_, awake)| !**awake)
            .map(|(name, _)| name)
            .collect();
        if !sleeping.is_empty() {
            defects.push(("D-SLP".into(), 3, format!("休眠中的尿袋: {}", sleeping.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
        }

        if metrics.atom_count < 5 {
            defects.push(("D-ATM".into(), 7, format!("原子能力不足: {}个（建议>10）", metrics.atom_count)));
        }

        if metrics.evolution_total > 100 && metrics.evolution_promoted == 0 {
            defects.push(("D-EVO".into(), 5, "有演化记录但无晋升，可能缺乏重复使用".into()));
        }

        if metrics.total_memory == 0 {
            defects.push(("D-MEM".into(), 6, "总记忆为零，系统没有积累经验".into()));
        }

        if metrics.pouch_count > 0 {
            let utilization = metrics.atom_count as f64 / (metrics.pouch_count as f64 * 3.0);
            if utilization < 0.3 {
                defects.push(("D-UTL".into(), 4, format!("尿袋利用率低: 平均每个尿袋仅{:.1}个原子", metrics.atom_count as f64 / metrics.pouch_count as f64)));
            }
        }

        if defects.is_empty() {
            return "=== 缺陷扫描 ===\n未发现缺陷。系统状态良好。".to_string();
        }

        defects.sort_by(|a, b| b.1.cmp(&a.1));

        let mut result = format!("=== 缺陷扫描 ===\n发现 {} 个问题:\n", defects.len());
        for (id, sev, desc) in &defects {
            let sev_label = if *sev >= 8 { "严重" } else if *sev >= 5 { "中等" } else { "轻微" };
            result.push_str(&format!("[{}] {}({}): {}\n", id, sev_label, sev, desc));
        }
        result
    }
}

#[derive(serde::Deserialize)]
struct SystemState {
    pattern_count: usize,
    pouch_count: usize,
    installed_pouches: Vec<String>,
    pouch_awake: Vec<bool>,
    atom_count: usize,
    evolution_total: usize,
    evolution_promoted: usize,
    total_memory: usize,
}

impl DefectScannerPouch {
    pub fn recommended_follow_ups(&self, my_output: &str) -> Vec<String> {
        if !my_output.contains("缺少核心尿袋") {
            return vec![];
        }
        vec!["reasoning".into(), "memory".into(), "context".into(), "programming".into()]
    }
}

#[async_trait]
impl Pouch for DefectScannerPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let input = &proposal.inner().content;
        let lower = input.to_lowercase();
        for (tokens, response) in &self.learned {
            let hits = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if hits >= 2 { return Ok(PouchOutput { data: response.clone(), confidence: 0.85 }); }
        }
        Ok(PouchOutput { data: self.scan_real(input), confidence: 0.95 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && tokens.len() >= 2 {
                let dominated = content.contains("缺陷") || content.contains("扫描")
                    || content.contains("问题") || content.contains("修复")
                    || content.contains("诊断") || content.contains("异常")
                    || content.contains("故障") || content.contains("检查")
                    || *weight >= 1.2;
                if dominated && !self.learned.iter().any(|(t, _)| t == tokens) {
                    self.learned.push((tokens.clone(), content.clone()));
                    if self.learned.len() > 200 { self.learned.remove(0); }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.learned.len() }
    fn explain(&self) -> String {
        format!("DefectScannerPouch: 缺陷扫描，学习{}条", self.learned.len())
    }
    fn recommended_follow_ups(&self, my_output: &str) -> Vec<String> {
        DefectScannerPouch::recommended_follow_ups(self, my_output)
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "defect_scan".into(),
            kind: AtomKind::Validate,
            pouch: self.name.clone(),
            confidence_range: (0.8, 0.95),
        }]
    }
}
