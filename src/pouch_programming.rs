use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct ProgrammingPouch {
    name: String,
    validator: ProposalValidator,
    learned: Vec<(Vec<String>, String)>,
}

impl ProgrammingPouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["code_generation".to_string(), "pipeline_data".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
            learned: Vec::new(),
        }
    }

    fn generate_code_template(&self, content: &str) -> String {
        let lower = content.to_lowercase();
        if lower.contains("代码分析") || lower.contains("code_analyzer") {
            "已生成代码分析器模板".to_string()
        } else if lower.contains("知识检索") || lower.contains("knowledge") {
            "已生成知识检索器模板".to_string()
        } else if lower.contains("推理") || lower.contains("reasoning") {
            "已生成推理增强器模板".to_string()
        } else {
            "编程尿袋就绪。请描述需要的功能。".to_string()
        }
    }
}

#[async_trait]
impl Pouch for ProgrammingPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let input = &proposal.inner().content;
        let lower = input.to_lowercase();
        for (tokens, response) in &self.learned {
            let hits = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if hits >= 2 {
                return Ok(PouchOutput { data: response.clone(), confidence: 0.88 });
            }
        }
        let result = self.generate_code_template(input);
        Ok(PouchOutput { data: result, confidence: 0.9 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && tokens.len() >= 2 {
                let dominated = content.contains("实现") || content.contains("代码")
                    || content.contains("fn ") || content.contains("struct ")
                    || content.contains("写") || content.contains("生成")
                    || content.contains("程序") || content.contains("算法")
                    || content.contains("编程") || content.contains("函数");
                if (dominated || *weight >= 1.2) && !self.learned.iter().any(|(t, _)| t == tokens) {
                    self.learned.push((tokens.clone(), content.clone()));
                    if self.learned.len() > 200 { self.learned.remove(0); }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.learned.len() }
    fn explain(&self) -> String { format!("ProgrammingPouch: 编程尿袋，已学{}条", self.learned.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "code_template".into(),
            kind: AtomKind::Generate,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.95),
        }]
    }
}
