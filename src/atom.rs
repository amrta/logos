#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AtomKind {
    Transform,
    Match,
    Score,
    Generate,
    Validate,
    Route,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AtomDeclaration {
    pub name: String,
    pub kind: AtomKind,
    pub pouch: String,
    pub confidence_range: (f64, f64),
}

pub struct CapabilityRegistry {
    atoms: Vec<AtomDeclaration>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        Self { atoms: Vec::new() }
    }

    pub fn register(&mut self, decl: AtomDeclaration) {
        for existing in &self.atoms {
            if existing.name == decl.name && existing.pouch == decl.pouch {
                return;
            }
        }
        self.atoms.push(decl);
    }

    pub fn unregister_pouch(&mut self, pouch_name: &str) {
        self.atoms.retain(|a| a.pouch != pouch_name);
    }

    pub fn find_by_kind(&self, kind: AtomKind) -> Vec<&AtomDeclaration> {
        self.atoms.iter().filter(|a| a.kind == kind).collect()
    }

    pub fn find_by_name(&self, name: &str) -> Option<&AtomDeclaration> {
        self.atoms.iter().find(|a| a.name == name)
    }

    pub fn all(&self) -> &[AtomDeclaration] {
        &self.atoms
    }

    pub fn count(&self) -> usize {
        self.atoms.len()
    }

    pub fn summary(&self) -> String {
        if self.atoms.is_empty() {
            return "原子能力注册表为空".into();
        }
        let mut lines = vec![format!("原子能力: {} 个", self.atoms.len())];
        for a in &self.atoms {
            lines.push(format!(
                "  {} ({:?}) ← {} [{:.0}%-{:.0}%]",
                a.name, a.kind, a.pouch,
                a.confidence_range.0 * 100.0,
                a.confidence_range.1 * 100.0
            ));
        }
        lines.join("\n")
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionStep>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExecutionStep {
    pub atom_name: String,
    pub pouch: String,
    pub kind: AtomKind,
    pub input_from: StepInput,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum StepInput {
    UserInput,
    PreviousStep(usize),
}

impl CapabilityRegistry {
    pub fn plan_for_kinds(
        &self,
        kinds: &[AtomKind],
        pouch_score: Option<&dyn Fn(&str) -> f64>,
        low_score_threshold: Option<f64>,
    ) -> Option<ExecutionPlan> {
        let threshold = low_score_threshold.unwrap_or(0.5);
        let mut steps = Vec::new();
        for (i, kind) in kinds.iter().enumerate() {
            let mut candidates = self.find_by_kind(*kind);
            let best = if let Some(score_fn) = pouch_score {
                candidates.retain(|a| score_fn(&a.pouch) >= threshold);
                if candidates.is_empty() {
                    let fallback = self.find_by_kind(*kind);
                    (*fallback.iter().max_by(|a, b| {
                        a.confidence_range.1.partial_cmp(&b.confidence_range.1).unwrap_or(std::cmp::Ordering::Equal)
                    })?).clone()
                } else {
                    candidates.sort_by(|a, b| {
                        let sa = score_fn(&a.pouch);
                        let sb = score_fn(&b.pouch);
                        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
                            .then_with(|| b.confidence_range.1.partial_cmp(&a.confidence_range.1).unwrap_or(std::cmp::Ordering::Equal))
                    });
                    (*candidates.first()?).clone()
                }
            } else {
                (*candidates.iter().max_by(|a, b| {
                    a.confidence_range.1.partial_cmp(&b.confidence_range.1).unwrap_or(std::cmp::Ordering::Equal)
                })?).clone()
            };
            steps.push(ExecutionStep {
                atom_name: best.name.clone(),
                pouch: best.pouch.clone(),
                kind: *kind,
                input_from: if i == 0 { StepInput::UserInput } else { StepInput::PreviousStep(i - 1) },
            });
        }
        if steps.is_empty() {
            return None;
        }
        Some(ExecutionPlan { steps })
    }

    pub fn plan_summary(plan: &ExecutionPlan) -> String {
        let mut parts = Vec::new();
        for (i, step) in plan.steps.iter().enumerate() {
            let input = match &step.input_from {
                StepInput::UserInput => "input".into(),
                StepInput::PreviousStep(n) => format!("step{}", n),
            };
            parts.push(format!("[{}] {}({})←{}", i, step.atom_name, step.pouch, input));
        }
        parts.join(" → ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_basic() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "math_eval".into(),
            kind: AtomKind::Score,
            pouch: "reasoning".into(),
            confidence_range: (0.5, 0.95),
        });
        assert_eq!(reg.count(), 1);
        assert!(reg.find_by_name("math_eval").is_some());
        assert_eq!(reg.find_by_kind(AtomKind::Score).len(), 1);
        reg.unregister_pouch("reasoning");
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn test_registry_no_duplicate() {
        let mut reg = CapabilityRegistry::new();
        let decl = AtomDeclaration {
            name: "transform_a".into(),
            kind: AtomKind::Transform,
            pouch: "material".into(),
            confidence_range: (0.6, 0.9),
        };
        reg.register(decl.clone());
        reg.register(AtomDeclaration {
            name: "transform_a".into(),
            kind: AtomKind::Transform,
            pouch: "material".into(),
            confidence_range: (0.6, 0.9),
        });
        assert_eq!(reg.count(), 1);
    }

    #[test]
    fn test_find_by_kind() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "a".into(), kind: AtomKind::Transform, pouch: "p1".into(), confidence_range: (0.5, 0.9),
        });
        reg.register(AtomDeclaration {
            name: "b".into(), kind: AtomKind::Score, pouch: "p2".into(), confidence_range: (0.5, 0.9),
        });
        reg.register(AtomDeclaration {
            name: "c".into(), kind: AtomKind::Transform, pouch: "p3".into(), confidence_range: (0.5, 0.9),
        });
        assert_eq!(reg.find_by_kind(AtomKind::Transform).len(), 2);
        assert_eq!(reg.find_by_kind(AtomKind::Score).len(), 1);
        assert_eq!(reg.find_by_kind(AtomKind::Generate).len(), 0);
    }

    #[test]
    fn test_plan_for_kinds_no_history_uses_confidence() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "high".into(),
            kind: AtomKind::Match,
            pouch: "pouch_high".into(),
            confidence_range: (0.8, 0.95),
        });
        reg.register(AtomDeclaration {
            name: "low".into(),
            kind: AtomKind::Match,
            pouch: "pouch_low".into(),
            confidence_range: (0.5, 0.7),
        });
        let plan = match reg.plan_for_kinds(&[AtomKind::Match], None, None) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].pouch, "pouch_high");
    }

    #[test]
    fn test_plan_for_kinds_with_scorer_prefers_high_score() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "high".into(),
            kind: AtomKind::Match,
            pouch: "pouch_high".into(),
            confidence_range: (0.8, 0.95),
        });
        reg.register(AtomDeclaration {
            name: "low".into(),
            kind: AtomKind::Match,
            pouch: "pouch_low".into(),
            confidence_range: (0.5, 0.7),
        });
        let scorer = |name: &str| if name == "pouch_low" { 10.0 } else { 0.0 };
        let plan = match reg.plan_for_kinds(&[AtomKind::Match], Some(&scorer), None) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].pouch, "pouch_low");
    }

    #[test]
    fn test_plan_for_kinds_with_low_score_filtered() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "low".into(),
            kind: AtomKind::Match,
            pouch: "pouch_low".into(),
            confidence_range: (0.9, 0.95),
        });
        reg.register(AtomDeclaration {
            name: "a".into(),
            kind: AtomKind::Match,
            pouch: "pouch_a".into(),
            confidence_range: (0.5, 0.8),
        });
        reg.register(AtomDeclaration {
            name: "b".into(),
            kind: AtomKind::Match,
            pouch: "pouch_b".into(),
            confidence_range: (0.5, 0.7),
        });
        let scorer = |name: &str| {
            if name == "pouch_low" { 0.3 } else if name == "pouch_a" { 2.0 } else { 2.0 }
        };
        let plan = match reg.plan_for_kinds(&[AtomKind::Match], Some(&scorer), None) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan.steps.len(), 1);
        assert_ne!(plan.steps[0].pouch, "pouch_low");
        assert!(plan.steps[0].pouch == "pouch_a" || plan.steps[0].pouch == "pouch_b");
    }

    #[test]
    fn test_plan_for_kinds_new_pouch_gets_baseline() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "new".into(),
            kind: AtomKind::Match,
            pouch: "pouch_new".into(),
            confidence_range: (0.8, 0.95),
        });
        reg.register(AtomDeclaration {
            name: "old".into(),
            kind: AtomKind::Match,
            pouch: "pouch_old".into(),
            confidence_range: (0.5, 0.7),
        });
        let scorer = |name: &str| if name == "pouch_new" { 1.0 } else { 1.0 };
        let plan = match reg.plan_for_kinds(&[AtomKind::Match], Some(&scorer), None) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].pouch, "pouch_new");
    }

    #[test]
    fn test_plan_for_kinds_configurable_threshold() {
        let mut reg = CapabilityRegistry::new();
        reg.register(AtomDeclaration {
            name: "a".into(),
            kind: AtomKind::Match,
            pouch: "pouch_a".into(),
            confidence_range: (0.5, 0.6),
        });
        reg.register(AtomDeclaration {
            name: "b".into(),
            kind: AtomKind::Match,
            pouch: "pouch_b".into(),
            confidence_range: (0.5, 0.7),
        });
        reg.register(AtomDeclaration {
            name: "c".into(),
            kind: AtomKind::Match,
            pouch: "pouch_c".into(),
            confidence_range: (0.5, 0.8),
        });
        let scorer = |name: &str| match name {
            "pouch_a" => 0.3,
            "pouch_b" => 0.4,
            _ => 2.0,
        };
        let plan_035 = match reg.plan_for_kinds(&[AtomKind::Match], Some(&scorer), Some(0.35)) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan_035.steps[0].pouch, "pouch_c");
        let plan_05 = match reg.plan_for_kinds(&[AtomKind::Match], Some(&scorer), Some(0.5)) {
            Some(p) => p,
            None => panic!("plan: None"),
        };
        assert_eq!(plan_05.steps[0].pouch, "pouch_c");
    }
}
