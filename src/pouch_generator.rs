use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct GeneratorPouch {
    name: String,
    validator: ProposalValidator,
    templates: Vec<(Vec<String>, String)>,
}

impl GeneratorPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["generate".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            templates: vec![],
        }
    }
}

#[async_trait]
impl Pouch for GeneratorPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (triggers, tpl) in &self.templates {
            if triggers.iter().filter(|t| content.contains(t.as_str())).count() >= 1 {
                let out = tpl.replace("{{input}}", &proposal.inner().content);
                return Ok(PouchOutput {
                    data: out,
                    confidence: 0.8,
                });
            }
        }
        Ok(PouchOutput {
            data: proposal.inner().content.clone(),
            confidence: 0.5,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.9 && content.contains("{{input}}") && self.templates.len() < 100 {
                self.templates.push((tokens.clone(), content.clone()));
            }
        }
    }
    fn memory_count(&self) -> usize { self.templates.len() }
    fn explain(&self) -> String { format!("GeneratorPouch: 生成模板，{}条", self.templates.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "generate".into(),
            kind: AtomKind::Generate,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.95),
        }]
    }
}
