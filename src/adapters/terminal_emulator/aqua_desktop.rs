use super::desktop::Desktop;

pub struct AquaDesktop;

impl AquaDesktop {
    const REFUSAL: &'static str =
        "Ringleader opens terminal windows and web browsers only on Linux";

    pub fn current() -> Desktop {
        Desktop::new(Some(Self::REFUSAL))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_macos_desktop_refuses_without_naming_a_display_variable() {
        let desktop = AquaDesktop::current();

        let refusal = desktop.refusal().unwrap();

        assert!(!refusal.contains("DISPLAY"));
    }
}
