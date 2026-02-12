use std::collections::HashMap;
use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct CodeTemplatePouch {
    name: String,
    validator: ProposalValidator,
    templates: HashMap<String, String>,
}

impl CodeTemplatePouch {
    pub fn new(name: &str) -> Self {
        let mut templates = HashMap::new();
        templates.insert("循环".into(), "for (let i = 0; i < n; i++) { }".into());
        templates.insert("函数".into(), "function fn() { return null; }".into());
        templates.insert("条件".into(), "if (condition) { }".into());
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["code_template".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            templates,
        }
    }
}

#[async_trait]
impl Pouch for CodeTemplatePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.to_lowercase();
        for (key, tpl) in &self.templates {
            if content.contains(key) || content.contains(&key.to_lowercase()) {
                return Ok(PouchOutput {
                    data: tpl.clone(),
                    confidence: 0.85,
                });
            }
        }
        Ok(PouchOutput {
            data: "未找到匹配的代码模板".to_string(),
            confidence: 0.3,
        })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, w) in patterns {
            if *w >= 0.8 && (content.contains("{") || content.contains("function") || content.contains("fn "))
                && self.templates.len() < 150
            {
                for t in tokens {
                    if t.chars().count() >= 2 && !self.templates.contains_key(t) {
                        self.templates.insert(t.clone(), content.clone());
                        break;
                    }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.templates.len() }
    fn explain(&self) -> String { format!("CodeTemplatePouch: 代码模板，{}条", self.templates.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "code_template".into(),
            kind: AtomKind::Generate,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.9),
        }]
    }
}
