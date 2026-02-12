use crate::pouch_trait::{
    ChemistryPouch, CloudTrainerPouch, ContextAwarePouch, CreativePouch, DiscoveryPouch,
    MaterialPouch, MemoryPouch, Pouch, PouchRole, PrinterPouch,
    ReasoningPouch,
};

const GATEWAY_BASE: &str = "https://logos-gateway.amrta.workers.dev";

pub fn instantiate(name: &str, data_dir: &str) -> Option<(Box<dyn Pouch>, PouchRole)> {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name == "language" {
        return None;
    }

    let cloud_pouches = [
        "image_generator", "cloud_general",
    ];
    if cloud_pouches.contains(&name.as_str()) {
        let endpoint = format!("{}/pouch/{}", GATEWAY_BASE, name);
        let rp = crate::remote_pouch::RemotePouch::new(&name, PouchRole::E1, &endpoint);
        return Some((Box::new(rp), PouchRole::E1));
    }

    let (pouch, role): (Box<dyn Pouch>, PouchRole) =
        if name.contains("benchmark") || name.contains("基准") {
            (Box::new(crate::pouch_benchmark::BenchmarkPouch::new()), PouchRole::E1)
        } else if name.contains("defect") || name.contains("缺陷") {
            (Box::new(crate::pouch_defect_scanner::DefectScannerPouch::new()), PouchRole::E1)
        } else if name.contains("comparer") || name.contains("对比") {
            (Box::new(crate::pouch_capability_comparer::CapabilityComparerPouch::new()), PouchRole::E1)
        } else if name.contains("code") || name.contains("代码分析") {
            (Box::new(crate::pouch_code_analyzer::CodeAnalyzerPouch::new()), PouchRole::E1)
        } else if name.contains("knowledge") || name.contains("知识") {
            (Box::new(crate::pouch_knowledge_retriever::KnowledgeRetrieverPouch::new()), PouchRole::E1)
        } else if name.contains("material") || name.contains("材料") {
            (Box::new(MaterialPouch::new(&name)), PouchRole::E1)
        } else if name.contains("print") || name.contains("打印") {
            (Box::new(PrinterPouch::new(&name)), PouchRole::E2)
        } else if name.contains("programming") || name.contains("编程") {
            (Box::new(crate::pouch_programming::ProgrammingPouch::new(&name)), PouchRole::E1)
        } else if name.contains("reason") || name.contains("推理") || name.contains("逻辑") {
            (Box::new(ReasoningPouch::new(&name)), PouchRole::E1)
        } else if name.contains("memory") || name.contains("记忆") || name.contains("回忆") {
            match MemoryPouch::new(&name, data_dir) {
                Ok(pouch) => (Box::new(pouch), PouchRole::E1),
                Err(_) => return None,
            }
        } else if name.contains("creative") || name.contains("创造") || name.contains("创意") {
            (Box::new(CreativePouch::new(&name)), PouchRole::E1)
        } else if name.contains("cloud_trainer") || name.contains("训练") {
            (Box::new(CloudTrainerPouch::new(&name)), PouchRole::E1)
        } else if name.contains("discovery") || name.contains("发现") || name.contains("容灾") {
            (Box::new(DiscoveryPouch::new(&name)), PouchRole::E1)
        } else if name.contains("context") || name.contains("消歧") || name.contains("上下文") {
            (Box::new(ContextAwarePouch::new(&name)), PouchRole::E1)
        } else if name.contains("chemistry") || name.contains("化学") || name.contains("分子") {
            (Box::new(ChemistryPouch::new(&name)), PouchRole::E1)
        } else if name.contains("pilot") || name.contains("试点") {
            (Box::new(crate::pouch_pilot::PilotPouch::new()), PouchRole::E1)
        } else {
            let endpoint = format!("{}/pouch/{}", GATEWAY_BASE, name);
            (Box::new(crate::remote_pouch::RemotePouch::new(&name, PouchRole::E1, &endpoint)), PouchRole::E1)
        };

    Some((pouch, role))
}
