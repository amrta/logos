use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct InductionPouch {
    name: String,
    validator: ProposalValidator,
    rules: Vec<(Vec<String>, String)>,
}

impl InductionPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["induction".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            rules: vec![],
        }
    }
}

#[async_trait]
impl Pouch for InductionPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (triggers, conclusion) in &self.rules {
            if triggers.iter().filter(|t| content.contains(t.as_str())).count() >= triggers.len() / 2 + 1 {
                return Ok(PouchOutput {
                    data: conclusion.clone(),
                    confidence: 0.82,
                });
            }
        }
        Ok(PouchOutput {
            data: "归纳规则未匹配".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.9 && tokens.len() >= 2 && !content.is_empty() && self.rules.len() < 100 {
                self.rules.push((tokens.clone(), content.clone()));
            }
        }
    }
    fn memory_count(&self) -> usize { self.rules.len() }
    fn explain(&self) -> String { format!("InductionPouch: 归纳规则，{}条", self.rules.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "induction".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}
