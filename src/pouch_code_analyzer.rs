use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct CodeAnalyzerPouch {
    name: String,
    validator: ProposalValidator,
    learned: Vec<(Vec<String>, String)>,
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
            learned: Vec::new(),
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
        let input = &proposal.inner().content;
        let lower = input.to_lowercase();
        for (tokens, response) in &self.learned {
            let hits = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if hits >= 2 {
                return Ok(PouchOutput { data: response.clone(), confidence: 0.82 });
            }
        }
        let result = self.analyze_code(input);
        Ok(PouchOutput { data: result, confidence: 0.85 })
    }
    fn sync_patterns(&mut self, patterns: &[(Vec<String>, String, f64)]) {
        for (tokens, content, weight) in patterns {
            if *weight >= 0.8 && tokens.len() >= 2 {
                let dominated = content.contains("代码") || content.contains("fn ")
                    || content.contains("分析") || content.contains("函数")
                    || content.contains("复杂度") || content.contains("bug")
                    || content.contains("程序") || content.contains("调试")
                    || content.contains("错误") || content.contains("优化");
                if (dominated || *weight >= 1.2) && !self.learned.iter().any(|(t, _)| t == tokens) {
                    self.learned.push((tokens.clone(), content.clone()));
                    if self.learned.len() > 200 { self.learned.remove(0); }
                }
            }
        }
    }
    fn memory_count(&self) -> usize { self.learned.len() }
    fn explain(&self) -> String { format!("CodeAnalyzerPouch: 代码分析，已学{}条", self.learned.len()) }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "code_analyze".into(),
            kind: AtomKind::Validate,
            pouch: self.name.clone(),
            confidence_range: (0.7, 0.9),
        }]
    }
}
