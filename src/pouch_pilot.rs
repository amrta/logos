/*
 * 此 Pouch 迁移自 Phase 试点，原目标：验证路由与演化链、补尿袋短板；
 * 当前适配：最小可执行形态，单 Transform 原子，供 e2e/路由偏好测试用。
 */
use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchRole, PouchOutput, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;

pub struct PilotPouch {
    name: String,
    validator: ProposalValidator,
}

impl PilotPouch {
    pub fn new() -> Self {
        Self {
            name: "pilot".to_string(),
            validator: ProposalValidator {
                allowed_types: vec!["pipeline_data".to_string(), "compare".to_string()],
                min_confidence: 0.0,
                min_evidence_count: 0,
            },
        }
    }
}

#[async_trait]
impl Pouch for PilotPouch {
    fn name(&self) -> &str {
        &self.name
    }
    fn role(&self) -> PouchRole {
        PouchRole::E1
    }
    fn validator(&self) -> &ProposalValidator {
        &self.validator
    }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let s = proposal.inner().content.as_str();
        let out = if s.is_empty() {
            "Pilot: 无输入".to_string()
        } else {
            format!("Pilot 已处理: {} ({} 字)", s.chars().take(30).collect::<String>(), s.chars().count())
        };
        Ok(PouchOutput {
            data: out,
            confidence: 0.85,
        })
    }
    fn memory_count(&self) -> usize {
        0
    }
    fn explain(&self) -> String {
        "PilotPouch: 试点迁移，最小可执行形态".into()
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        vec![AtomDeclaration {
            name: "pilot_transform".into(),
            kind: AtomKind::Transform,
            pouch: self.name.clone(),
            confidence_range: (0.5, 0.8),
        }]
    }
}
