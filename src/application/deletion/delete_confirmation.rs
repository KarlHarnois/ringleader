use super::delete_impact::DeleteImpact;
use crate::application::ServiceError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteConfirmation {
    seen: Option<DeleteImpact>,
    forget: bool,
}

impl DeleteConfirmation {
    pub fn confirmed(seen: DeleteImpact) -> Self {
        Self {
            seen: Some(seen),
            forget: false,
        }
    }

    pub fn force() -> Self {
        Self {
            seen: None,
            forget: false,
        }
    }

    pub fn forgetting(self) -> Self {
        Self {
            forget: true,
            ..self
        }
    }

    pub fn forgets(&self) -> bool {
        self.forget
    }

    pub fn covers_working(&self) -> bool {
        self.seen.as_ref().is_none_or(|seen| seen.working())
    }

    pub fn check(&self, current: &DeleteImpact) -> Result<(), ServiceError> {
        let Some(seen) = &self.seen else {
            return Ok(());
        };
        if seen.accepts(current) {
            return Ok(());
        }
        Err(ServiceError::GuardsChanged {
            message: format!("delete would lose work: {}", current.warnings().join("; ")),
            impact: Box::new(current.clone()),
        })
    }
}
