use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct AnalogyPouch {
    name: String,
    validator: ProposalValidator,
    mappings: Vec<((String, String), (String, String))>,
}

impl AnalogyPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["analogy".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            mappings: vec![
                (("鸟".into(), "飞".into()), ("鱼".into(), "游".into())),
                (("热".into(), "夏天".into()), ("冷".into(), "冬天".into())),
            ],
        }
    }
}

#[async_trait]
impl Pouch for AnalogyPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for ((a, b), (c, d)) in &self.mappings {
            if content.contains(a) && content.contains(b) {
                return Ok(PouchOutput {
                    data: format!("{}:{} 类比于 {}:{}", a, b, c, d),
                    confidence: 0.85,
                });
            }
        }
        Ok(PouchOutput {
            data: "未找到匹配的类比模式".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (_tokens, content, w) in patterns {
            if *w >= 0.9 && content.contains(":") && content.contains("类比") && self.mappings.len() < 100 {
                let parts: Vec<&str> = content.split(':').collect();
                if parts.len() >= 4 {
                    let a = parts[0].trim().to_string();
                    let b = parts[1].trim().to_string();
                    let c = parts[2].trim().to_string();
                    let d = parts[3].trim().to_string();
                    if !a.is_empty() && !b.is_empty() && !c.is_empty() && !d.is_empty() {
                        self.mappings.push(((a, b), (c, d)));
                    }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.mappings.len() }
    fn explain(&self) -> String { format!("AnalogyPouch: 类比模式匹配，{}条", self.mappings.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "analogy_match".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}
