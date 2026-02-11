use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct ProgrammingPouch {
    name: String,
    validator: ProposalValidator,
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
        let result = self.generate_code_template(&proposal.inner().content);
        Ok(PouchOutput { data: result, confidence: 0.9 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "ProgrammingPouch: 编程尿袋，生成代码模板".into() }
}
