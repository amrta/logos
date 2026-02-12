use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct ExplorerPouch {
    name: String,
    validator: ProposalValidator,
    uncovered_samples: Vec<String>,
}

impl ExplorerPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["explore".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            uncovered_samples: Vec::new(),
        }
    }
}

#[async_trait]
impl Pouch for ExplorerPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.trim();
        if content.len() >= 5 && !self.uncovered_samples.iter().any(|s| s == content) {
            if self.uncovered_samples.len() >= 200 {
                self.uncovered_samples.remove(0);
            }
            self.uncovered_samples.push(content.to_string());
        }
        let count = self.uncovered_samples.len();
        Ok(PouchOutput {
            data: format!("探索袋已记录{}条未覆盖样本，可触发反哺", count),
            confidence: 0.9,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.9 && content.contains("未覆盖") && self.uncovered_samples.len() < 200 {
                let s = tokens.join(" ");
                if s.len() >= 5 && !self.uncovered_samples.contains(&s) {
                    self.uncovered_samples.push(s);
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.uncovered_samples.len() }
    fn explain(&self) -> String { format!("ExplorerPouch: 长尾探索，{}条未覆盖样本", self.uncovered_samples.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "explore".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.95),
        }]
    }
}
