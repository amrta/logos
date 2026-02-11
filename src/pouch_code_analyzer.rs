use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct CodeAnalyzerPouch {
    name: String,
    validator: ProposalValidator,
}

impl CodeAnalyzerPouch {
    pub fn new() -> Self {
        Self {
            name: "code_analyzer".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["code_analysis".to_string(), "pipeline_data".to_string(), "analyze".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
        }
    }

    fn analyze_code(&self, code: &str) -> String {
        let mut issues = Vec::new();
        if code.contains("unwrap()") {
            issues.push("unwrap() 调用 - 可能 panic");
        }
        if code.contains("TODO") || code.contains("FIXME") {
            issues.push("未完成的代码标记");
        }
        if code.len() > 1000 {
            issues.push("代码过长 - 考虑分解");
        }
        if issues.is_empty() {
            "代码分析通过".to_string()
        } else {
            format!("发现 {} 个问题:\n{}", issues.len(), issues.join("\n"))
        }
    }
}

#[async_trait]
impl Pouch for CodeAnalyzerPouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E1 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let result = self.analyze_code(&proposal.inner().content);
        Ok(PouchOutput { data: result, confidence: 0.85 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "CodeAnalyzerPouch: 代码分析尿袋".into() }
}
