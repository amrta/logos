use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct RealtimePouch {
    name: String,
    validator: ProposalValidator,
    cache: Vec<(String, String)>,
}

impl RealtimePouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["realtime_query".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            cache: Vec::new(),
        }
    }
}

#[async_trait]
impl Pouch for RealtimePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (key, val) in &self.cache {
            if content.contains(key) || content.contains(&key.to_lowercase()) {
                return Ok(PouchOutput { data: val.clone(), confidence: 0.85 });
            }
        }
        Ok(PouchOutput {
            data: "实时袋：定时拉RSS/API解析为(human,gpt)，待扩展实时源".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.9 && content.len() <= 200 && self.cache.len() < 100 {
                let key = tokens.first().cloned().unwrap_or_default();
                if !key.is_empty() && !self.cache.iter().any(|(k, _)| k == &key) {
                    self.cache.push((key, content.clone()));
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.cache.len() }
    fn explain(&self) -> String { format!("RealtimePouch: 实时数据，{}条", self.cache.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "realtime_query".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}
