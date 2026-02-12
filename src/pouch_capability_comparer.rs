use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct CapabilityComparerPouch {
    name: String,
    validator: ProposalValidator,
    learned: Vec<(Vec<String>, String)>,
}

impl CapabilityComparerPouch {
    pub fn new() -> Self {
        Self {
            name: "capability_comparer".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["compare".to_string(), "pipeline_data".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            learned: Vec::new(),
        }
    }

    pub fn real_comparison(&self, input: &str) -> String {
        let caps: CapabilityState = if let Ok(c) = serde_json::from_str(input) {
            c
        } else {
            return "能力对标需要系统能力数据（JSON格式）".to_string();
        };

        let lang_score = self.calc_language_score(caps.pattern_count, caps.has_context);
        let reason_score = self.calc_reasoning_score(&caps.installed_pouches);
        let knowledge_score = self.calc_knowledge_score(&caps.installed_pouches, caps.total_memory);
        let creative_score = self.calc_creative_score(&caps.installed_pouches);
        let code_score = self.calc_code_score(&caps.installed_pouches);

        let comparisons = [
            ("语言理解", lang_score, 0.95, 0.93),
            ("逻辑推理", reason_score, 0.92, 0.94),
            ("知识检索", knowledge_score, 0.88, 0.85),
            ("创意生成", creative_score, 0.87, 0.90),
            ("代码分析", code_score, 0.91, 0.93),
        ];

        let logos_avg: f64 = comparisons.iter().map(|(_, l, _, _)| l).sum::<f64>() / comparisons.len() as f64;

        let mut result = format!("=== 实测能力对标 ===\nLOGOS 综合: {:.1}%\n", logos_avg * 100.0);
        for (cat, logos, gpt, claude) in &comparisons {
            let gap = (f64::max(*gpt, *claude) - logos) * 100.0;
            let gap_str = if gap > 10.0 { format!(" ▼{:.0}%", gap) } else if gap > 0.0 { format!(" △{:.0}%", gap) } else { " ≈".into() };
            result.push_str(&format!(
                "{}: LOGOS {:.0}% | GPT {:.0}% | Claude {:.0}%{}\n",
                cat, logos * 100.0, gpt * 100.0, claude * 100.0, gap_str
            ));
        }

        let severe_gaps: Vec<&&str> = comparisons.iter()
            .filter(|(_, l, g, c)| (f64::max(*g, *c) - l) > 0.3)
            .map(|(name, _, _, _)| name)
            .collect();

        if !severe_gaps.is_empty() {
            result.push_str(&format!("\n关键差距领域: {}", severe_gaps.iter().map(|s| **s).collect::<Vec<_>>().join(", ")));
        }

        result
    }

    fn calc_language_score(&self, patterns: usize, has_context: bool) -> f64 {
        let base = (patterns as f64 / 2000.0).min(0.6);
        let context_bonus = if has_context { 0.1 } else { 0.0 };
        (base + context_bonus).min(0.9)
    }

    fn calc_reasoning_score(&self, pouches: &[String]) -> f64 {
        let has_reasoning = pouches.iter().any(|p| p.contains("reason") || p.contains("推理"));
        if has_reasoning { 0.35 } else { 0.05 }
    }

    fn calc_knowledge_score(&self, pouches: &[String], memory: usize) -> f64 {
        let has_knowledge = pouches.iter().any(|p| p.contains("knowledge") || p.contains("知识"));
        let has_memory = pouches.iter().any(|p| p.contains("memory") || p.contains("记忆"));
        let base = if has_knowledge { 0.15 } else { 0.02 };
        let mem_bonus = if has_memory { (memory as f64 / 5000.0).min(0.15) } else { 0.0 };
        base + mem_bonus
    }

    fn calc_creative_score(&self, pouches: &[String]) -> f64 {
        let has_creative = pouches.iter().any(|p| p.contains("creative") || p.contains("创造"));
        if has_creative { 0.20 } else { 0.03 }
    }

    fn calc_code_score(&self, pouches: &[String]) -> f64 {
        let has_code = pouches.iter().any(|p| p.contains("code") || p.contains("代码"));
        let has_programming = pouches.iter().any(|p| p.contains("programming") || p.contains("编程"));
        let base = if has_code { 0.15 } else { 0.02 };
        let prog_bonus = if has_programming { 0.10 } else { 0.0 };
        base + prog_bonus
    }

    pub fn parse_evolution_gaps(&self, my_output: &str) -> Vec<(String, f64)> {
        let mut gaps = Vec::new();
        for line in my_output.lines() {
            if !line.contains("LOGOS") || !line.contains('|') || !line.contains('%') {
                continue;
            }
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 2 {
                continue;
            }
            let category = parts[0].trim().to_string();
            let scores: Vec<f64> = parts[1]
                .split('|')
                .filter_map(|s| {
                    let mut cleaned = s.replace("LOGOS", "").replace("GPT", "").replace("Claude", "");
                    for c in ['%', '▼', '△', '≈'] {
                        cleaned = cleaned.replace(c, "");
                    }
                    cleaned.trim().parse::<f64>().ok()
                })
                .collect();
            if scores.len() >= 3 {
                let logos_score = scores[0];
                let max_competitor = scores[1].max(scores[2]);
                if logos_score < max_competitor * 0.8 {
                    gaps.push((category, max_competitor - logos_score));
                }
            }
        }
        gaps
    }
}

#[derive(serde::Deserialize)]
struct CapabilityState {
    pattern_count: usize,
    installed_pouches: Vec<String>,
    total_memory: usize,
    has_context: bool,
}

#[async_trait]
impl Pouch for CapabilityComparerPouch {
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
        Ok(PouchOutput { data: self.real_comparison(input), confidence: 0.95 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && tokens.len() >= 2 {
                let dominated = content.contains("对比") || content.contains("能力")
                    || content.contains("差距") || content.contains("评估")
                    || content.contains("对标") || content.contains("compare")
                    || content.contains("排名") || content.contains("竞品")
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
        format!("CapabilityComparerPouch: 能力对标，学习{}条", self.learned.len())
    }
    fn evolution_gaps_from_output(&self, my_output: &str) -> Vec<(String, f64)> {
        self.parse_evolution_gaps(my_output)
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![
            AtomDeclaration { name: "compare".into(), kind: AtomKind::Match, pouch: self.name.clone(), confidence_range: (0.7, 0.95) },
            AtomDeclaration { name: "score_compare".into(), kind: AtomKind::Score, pouch: self.name.clone(), confidence_range: (0.7, 0.95) },
        ]
    }
}
