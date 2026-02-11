use crate::atom::{AtomDeclaration, AtomKind};
use crate::pouch_trait::{Pouch, PouchOutput, PouchRole, ProposalValidator, ValidatedProposal};
use async_trait::async_trait;
use std::time::Duration;

pub struct RemotePouch {
    pub name: String,
    pub role: PouchRole,
    pub endpoint: String,
    pub failover_endpoints: Vec<String>,
    validator: ProposalValidator,
}

impl RemotePouch {
    pub fn new(name: &str, role: PouchRole, endpoint: &str) -> Self {
        Self {
            name: name.to_string(),
            role,
            endpoint: endpoint.to_string(),
            failover_endpoints: Vec::new(),
            validator: ProposalValidator {
                allowed_types: vec!["generic".into(), "pipeline_data".into()],
                min_confidence: 0.1,
                min_evidence_count: 0,
            },
        }
    }

    async fn call_with_failover(&self, input: &str) -> String {
        let result = self.call_endpoint(&self.endpoint, input).await;
        if !result.contains("暂时不可用") {
            return result;
        }

        for ep in &self.failover_endpoints {
            let result = self.call_endpoint(ep, input).await;
            if !result.contains("暂时不可用") {
                return result;
            }
        }

        result
    }

    async fn call_endpoint(&self, endpoint: &str, input: &str) -> String {
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(c) => c,
            Err(_) => return "远程尿袋服务暂时不可用".into(),
        };
        let payload = serde_json::json!({ "input": input });
        let response = match client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(_) => return "远程尿袋服务暂时不可用".into(),
        };
        if !response.status().is_success() {
            return "远程尿袋服务暂时不可用".into();
        }
        match response.json::<serde_json::Value>().await {
            Ok(json) => {
                if let Some(result) = json.get("result").and_then(|v| v.as_str()) {
                    result.to_string()
                } else if let Some(output) = json.get("output").and_then(|v| v.as_str()) {
                    output.to_string()
                } else {
                    serde_json::to_string(&json).unwrap_or_else(|_| "远程尿袋服务暂时不可用".into())
                }
            }
            Err(_) => "远程尿袋服务暂时不可用".into(),
        }
    }
}

#[async_trait]
impl Pouch for RemotePouch {
    fn name(&self) -> &str { &self.name }
    fn role(&self) -> PouchRole { self.role }
    fn validator(&self) -> &ProposalValidator { &self.validator }
    async fn process_proposal(&mut self, proposal: &ValidatedProposal) -> Result<PouchOutput, String> {
        let result = self.call_with_failover(&proposal.inner().content).await;
        Ok(PouchOutput { data: result, confidence: 0.8 })
    }
    fn memory_count(&self) -> usize { 0 }
    fn explain(&self) -> String {
        let fo = if self.failover_endpoints.is_empty() { "" } else { " +failover" };
        format!("RemotePouch[{}]: 远程计算尿袋{}", self.name, fo)
    }
    fn atom_capabilities(&self) -> Vec<AtomDeclaration> {
        let n = self.name.as_str();
        let pouch = self.name.clone();
        match n {
            "reasoning" => vec![
                AtomDeclaration { name: "math_eval".into(), kind: AtomKind::Score, pouch: pouch.clone(), confidence_range: (0.5, 0.95) },
                AtomDeclaration { name: "logical_infer".into(), kind: AtomKind::Match, pouch: pouch.clone(), confidence_range: (0.4, 0.85) },
                AtomDeclaration { name: "comparison".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.5, 0.9) },
            ],
            "creative" => vec![
                AtomDeclaration { name: "creative_generate".into(), kind: AtomKind::Generate, pouch, confidence_range: (0.5, 0.85) },
            ],
            "memory" => vec![
                AtomDeclaration { name: "memory_store".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.7, 0.95) },
            ],
            "image_generator" => vec![
                AtomDeclaration { name: "image_generate".into(), kind: AtomKind::Generate, pouch, confidence_range: (0.6, 0.9) },
            ],
            "code_analyzer" => vec![
                AtomDeclaration { name: "code_analyze".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.6, 0.9) },
            ],
            "knowledge_retriever" => vec![
                AtomDeclaration { name: "knowledge_retrieve".into(), kind: AtomKind::Match, pouch, confidence_range: (0.5, 0.9) },
            ],
            "chemistry" => vec![
                AtomDeclaration { name: "molecule_analyze".into(), kind: AtomKind::Transform, pouch: pouch.clone(), confidence_range: (0.7, 0.95) },
                AtomDeclaration { name: "element_interact".into(), kind: AtomKind::Match, pouch, confidence_range: (0.5, 0.85) },
            ],
            "material_analyzer" => vec![
                AtomDeclaration { name: "material_analyze".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.7, 0.95) },
            ],
            "printer_3d" => vec![
                AtomDeclaration { name: "gcode_generate".into(), kind: AtomKind::Generate, pouch, confidence_range: (0.7, 0.95) },
            ],
            "discovery" => vec![
                AtomDeclaration { name: "service_scan".into(), kind: AtomKind::Match, pouch, confidence_range: (0.6, 0.9) },
            ],
            "cloud_general" => vec![
                AtomDeclaration { name: "general_transform".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.4, 0.8) },
            ],
            _ => vec![
                AtomDeclaration { name: "remote_transform".into(), kind: AtomKind::Transform, pouch, confidence_range: (0.3, 0.75) },
            ],
        }
    }
}

#[derive(serde::Deserialize, Clone)]
pub struct RemotePouchSpec {
    pub name: String,
    pub role: String,
    pub endpoint: String,
    #[serde(default)]
    pub failover_endpoints: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remote_pouch_failure() {
        let mut pouch = RemotePouch::new("test", PouchRole::E1, "http://invalid.local/x");
        let proposal = crate::pouch_trait::create_proposal("测试");
        let validated = match pouch.validator().validate(&proposal) {
            Ok(v) => v,
            Err(e) => panic!("validation: {}", e),
        };
        let result = pouch.process_proposal(&validated).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_failover_endpoints() {
        let mut pouch = RemotePouch::new("test", PouchRole::E1, "http://invalid.local/x");
        pouch.failover_endpoints.push("http://also-invalid.local/y".into());
        assert_eq!(pouch.failover_endpoints.len(), 1);
    }
}
