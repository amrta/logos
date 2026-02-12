use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct ComposePouch {
    name: String,
    validator: ProposalValidator,
}

impl ComposePouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["compose".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
        }
    }
}

#[async_trait]
impl Pouch for ComposePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let parts: Vec<&str> = proposal.inner().content.split('|').map(str::trim).filter(|s| !s.is_empty()).collect();
        let out = if parts.len() >= 2 {
            parts.join("；")
        } else {
            proposal.inner().content.clone()
        };
        Ok(PouchOutput {
            data: out,
            confidence: 0.9,
        })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "ComposePouch: 多输入合并，按|分隔组合".into() }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "compose".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.8, 1.0),
        }]
    }
}
