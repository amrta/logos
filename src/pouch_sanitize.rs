use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

const SUSPICIOUS: &[&str] = &[
    "忽略", "无视", "忘记", "忽略上述", "忽略之前", "重新开始",
];

pub struct SanitizePouch {
    name: String,
    validator: ProposalValidator,
}

impl SanitizePouch {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["sanitize".into(), "pipeline_data".into()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
        }
    }
}

#[async_trait]
impl Pouch for SanitizePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { PouchRole::E0 }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let content = proposal.inner().content.trim();
        if content.len() > 4096 {
            return Ok(PouchOutput {
                data: "输入超长，已截断".to_string(),
                confidence: 0.0,
            });
        }
        for s in SUSPICIOUS {
            if content.contains(s) {
                return Ok(PouchOutput {
                    data: "输入含可疑指令，已拒绝".to_string(),
                    confidence: 0.0,
                });
            }
        }
        Ok(PouchOutput {
            data: content.to_string(),
            confidence: 1.0,
        })
    }
    fn is_fallback_output(&self, output: &str) -> bool {
        output == "输入超长，已截断" || output == "输入含可疑指令，已拒绝"
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String { "SanitizePouch: 输入清洗，异常直接fallback".into() }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "sanitize".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.99, 1.0),
        }]
    }
}
