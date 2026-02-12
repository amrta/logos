use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct BenchmarkPouch {
    name: String,
    validator: ProposalValidator,
    learned: Vec<(Vec<String>, String)>,
}

impl BenchmarkPouch {
    pub fn new() -> Self {
        Self {
            name: "benchmark".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["benchmark".to_string(), "pipeline_data".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            learned: Vec::new(),
        }
    }

    pub fn run_benchmark(&self, input: &str) -> String {
        let metrics: SystemMetrics = if let Ok(m) = serde_json::from_str(input) {
            m
        } else {
            return "基准评估需要系统指标数据（JSON格式）".to_string();
        };

        let pattern_score = self.score_patterns(metrics.pattern_count);
        let pouch_score = self.score_pouches(metrics.pouch_count, metrics.max_pouches);
        let atom_score = self.score_atoms(metrics.atom_count);
        let evolution_score = self.score_evolution(metrics.evolution_total, metrics.evolution_promoted);
        let memory_score = self.score_memory(metrics.total_memory);

        let overall = (pattern_score + pouch_score + atom_score + evolution_score + memory_score) / 5.0;

        let mut result = format!("=== 实测基准 ===\n整体: {:.1}%\n", overall * 100.0);
        result.push_str(&format!("语言模式: {:.1}% ({}条, 目标8000)\n", pattern_score * 100.0, metrics.pattern_count));
        result.push_str(&format!("尿袋覆盖: {:.1}% ({}/{} 已装)\n", pouch_score * 100.0, metrics.pouch_count, metrics.max_pouches));
        result.push_str(&format!("原子能力: {:.1}% ({}个)\n", atom_score * 100.0, metrics.atom_count));
        result.push_str(&format!("演化成熟度: {:.1}% ({}记录, {}晋升)\n", evolution_score * 100.0, metrics.evolution_total, metrics.evolution_promoted));
        result.push_str(&format!("记忆深度: {:.1}% ({}条)", memory_score * 100.0, metrics.total_memory));

        if overall < 0.3 {
            result.push_str("\n诊断: 系统处于早期阶段，需大量对话和尿袋扩展");
        } else if overall < 0.6 {
            result.push_str("\n诊断: 系统已有基础能力，建议继续扩展尿袋和积累模式");
        } else {
            result.push_str("\n诊断: 系统能力较成熟");
        }

        result
    }

    fn score_patterns(&self, count: usize) -> f64 {
        (count as f64 / 8000.0).min(1.0)
    }

    fn score_pouches(&self, installed: usize, max: usize) -> f64 {
        if max == 0 { return 0.0; }
        (installed as f64 / max as f64).min(1.0)
    }

    fn score_atoms(&self, count: usize) -> f64 {
        let target = 20.0;
        (count as f64 / target).min(1.0)
    }

    fn score_evolution(&self, total: usize, promoted: usize) -> f64 {
        let volume_score = (total as f64 / 500.0).min(0.5);
        let promotion_score = if total > 0 {
            ((promoted as f64 / total as f64) * 2.0).min(0.5)
        } else {
            0.0
        };
        volume_score + promotion_score
    }

    fn score_memory(&self, total: usize) -> f64 {
        (total as f64 / 1000.0).min(1.0)
    }
}

#[derive(serde::Deserialize)]
struct SystemMetrics {
    pattern_count: usize,
    pouch_count: usize,
    max_pouches: usize,
    atom_count: usize,
    evolution_total: usize,
    evolution_promoted: usize,
    total_memory: usize,
}

#[async_trait]
impl Pouch for BenchmarkPouch {
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
        Ok(PouchOutput { data: self.run_benchmark(input), confidence: 0.95 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && tokens.len() >= 2 {
                let dominated = content.contains("基准") || content.contains("评测")
                    || content.contains("性能") || content.contains("指标")
                    || content.contains("评估") || content.contains("benchmark")
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
        format!("BenchmarkPouch: 基准评估，学习{}条", self.learned.len())
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "system_benchmark".into(),
            kind: AtomKind::Score,
            pouch: self.name.clone(),
            confidence_range: (0.8, 0.95),
        }]
    }
}
