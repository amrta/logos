use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct DeductionPouch {
    name: String,
    validator: ProposalValidator,
    rules: Vec<(String, String)>,
}

impl DeductionPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["deduction".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            rules: vec![
                ("若A则B".into(), "A成立则B成立".into()),
                ("全称".into(), "所有实例满足则一般成立".into()),
            ],
        }
    }
}

#[async_trait]
impl Pouch for DeductionPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (premise, conclusion) in &self.rules {
            let pl = premise.to_lowercase();
            if content.contains(premise) || content.contains(&pl) {
                return Ok(PouchOutput {
                    data: conclusion.clone(),
                    confidence: 0.88,
                });
            }
        }
        Ok(PouchOutput {
            data: "演绎规则未匹配".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.9 && content.contains("则") && self.rules.len() < 80 {
                let s: String = tokens.join(" ");
                if s.len() >= 4 {
                    self.rules.push((s, content.clone()));
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.rules.len() }
    fn explain(&self) -> String { format!("DeductionPouch: 演绎规则，{}条", self.rules.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "deduction".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.95),
        }]
    }
}
