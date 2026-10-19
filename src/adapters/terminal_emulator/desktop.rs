#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Desktop {
    refusal: Option<String>,
}

impl Desktop {
    pub fn new(refusal: Option<&str>) -> Self {
        Self {
            refusal: refusal.map(str::to_string),
        }
    }

    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_built_with_a_refusal_keeps_it() {
        let desktop = Desktop::new(Some("example refusal"));

        assert_eq!(desktop.refusal(), Some("example refusal"));
    }

    #[test]
    fn a_desktop_built_without_a_refusal_refuses_nothing() {
        assert_eq!(Desktop::new(None).refusal(), None);
    }
}
