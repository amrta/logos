use std::collections::HashMap;
use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct KnowledgeRetrieverPouch {
    name: String,
    validator: ProposalValidator,
    knowledge_base: HashMap<String, String>,
}

impl KnowledgeRetrieverPouch {
    pub fn new() -> Self {
        let mut kb = HashMap::new();
        kb.insert("人工智能".to_string(), "通过计算机系统模拟人类智能的技术".to_string());
        kb.insert("机器学习".to_string(), "计算机系统通过经验改进性能的技术".to_string());
        kb.insert("深度学习".to_string(), "基于人工神经网络的机器学习方法".to_string());
        kb.insert("分形".to_string(), "自相似的几何图形或结构".to_string());
        Self {
            name: "knowledge_retriever".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["query".to_string(), "pipeline_data".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            knowledge_base: kb,
        }
    }

    fn search(&self, query: &str) -> String {
        for (key, value) in &self.knowledge_base {
            if query.contains(key) {
                return format!("{}: {}", key, value);
            }
        }
        "未找到相关知识".to_string()
    }
}

#[async_trait]
impl Pouch for KnowledgeRetrieverPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let result = self.search(&proposal.inner().content);
        Ok(PouchOutput { data: result, confidence: 0.85 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && !content.is_empty() && self.knowledge_base.len() < 500 {
                for token in tokens {
                    if token.chars().count() >= 2 && !self.knowledge_base.contains_key(token) {
                        self.knowledge_base.insert(token.clone(), content.clone());
                    }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.knowledge_base.len() }
    fn explain(&self) -> String { format!("KnowledgeRetrieverPouch: 知识检索，{}条", self.knowledge_base.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "knowledge_retrieve".into(),
            kind: AtomKind::Match,
            pouch: self.name.clone(),
            confidence_range: (0.6, 0.9),
        }]
    }
}
