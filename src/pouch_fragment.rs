use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct FragmentPouch {
    name: String,
    validator: ProposalValidator,
    fragments: Vec<(Vec<String>, String)>,
}

impl FragmentPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["fragment".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            fragments: vec![
                (vec!["开头".into(), "首先".into()], "首先，".into()),
                (vec!["结尾".into(), "总结".into()], "综上所述，".into()),
            ],
        }
    }
}

#[async_trait]
impl Pouch for FragmentPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (triggers, fragment) in &self.fragments {
            if triggers.iter().any(|t| content.contains(t.as_str())) {
                return Ok(PouchOutput {
                    data: fragment.clone(),
                    confidence: 0.85,
                });
            }
        }
        Ok(PouchOutput {
            data: "".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.85 && content.len() <= 80 && self.fragments.len() < 200 {
                self.fragments.push((tokens.clone(), content.clone()));
            }
        }
    }
    fn memory_count(&self) -> usize { self.fragments.len() }
    fn explain(&self) -> String { format!("FragmentPouch: 短语库，{}条", self.fragments.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "fragment_retrieve".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}
