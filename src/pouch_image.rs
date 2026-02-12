use std::collections::HashMap;
use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct ImagePouch {
    name: String,
    validator: ProposalValidator,
    index: HashMap<String, String>,
}

impl ImagePouch {
    pub fn new(name: &str) -> Self {
        let mut index = HashMap::new();
        index.insert("logo".into(), "图像检索:logo类型".into());
        index.insert("截图".into(), "图像检索:截图类型".into());
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["image_query".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            index,
        }
    }
}

#[async_trait]
impl Pouch for ImagePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (key, resp) in &self.index {
            if content.contains(key) || content.contains(&key.to_lowercase()) {
                return Ok(PouchOutput { data: resp.clone(), confidence: 0.8 });
            }
        }
        Ok(PouchOutput {
            data: "图像袋：未命中，存(描述,响应)检索".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.8 && self.index.len() < 100 {
                for t in tokens {
                    if t.chars().count() >= 2 && !self.index.contains_key(t) {
                        self.index.insert(t.clone(), content.clone());
                        break;
                    }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.index.len() }
    fn explain(&self) -> String { format!("ImagePouch: 图像描述检索，{}条", self.index.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "image_retrieve".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.85),
        }]
    }
}
