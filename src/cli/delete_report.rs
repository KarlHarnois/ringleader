use crate::application::{DeleteImpact, DeleteOutcome};

pub struct DeleteReport<'a> {
    name: &'a str,
}

impl<'a> DeleteReport<'a> {
    pub fn new(name: &'a str) -> Self {
        Self { name }
    }

    pub fn refusal(&self, impact: &DeleteImpact) -> Vec<String> {
        let mut lines = vec![format!("deleting {} would lose work:", self.name)];
        lines.extend(
            impact
                .warnings()
                .iter()
                .map(|warning| format!("  - {warning}")),
        );
        lines.push("pass --force to delete anyway".to_string());
        lines
    }

    pub fn warnings(&self, impact: &DeleteImpact) -> Vec<String> {
        impact
            .warnings()
            .iter()
            .map(|warning| format!("warning: {warning}"))
            .collect()
    }

    pub fn success(&self, outcome: &DeleteOutcome) -> String {
        outcome.summary(self.name)
    }
}
