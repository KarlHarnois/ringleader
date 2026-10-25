use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Lit, Meta, MetaList, MetaNameValue, Token};

#[derive(Clone, Copy)]
pub struct HostConfiguration {
    target_os: &'static str,
}

impl HostConfiguration {
    const CFG: &str = "cfg";
    const TEST: &str = "test";
    const TARGET_OS: &str = "target_os";

    pub fn current() -> Self {
        Self {
            target_os: std::env::consts::OS,
        }
    }

    #[cfg(test)]
    pub fn on(target_os: &'static str) -> Self {
        Self { target_os }
    }

    pub fn compiles(&self, attributes: &[Attribute]) -> bool {
        attributes
            .iter()
            .filter(|attribute| attribute.path().is_ident(Self::CFG))
            .all(|attribute| self.holds_attribute(attribute).unwrap_or(true))
    }

    fn holds_attribute(&self, attribute: &Attribute) -> Option<bool> {
        let predicate = attribute.parse_args::<Meta>().ok()?;
        self.holds(&predicate)
    }

    fn holds(&self, predicate: &Meta) -> Option<bool> {
        match predicate {
            Meta::Path(path) => path.is_ident(Self::TEST).then_some(false),
            Meta::NameValue(option) => self.holds_option(option),
            Meta::List(combinator) => self.holds_combinator(combinator),
        }
    }

    fn holds_option(&self, option: &MetaNameValue) -> Option<bool> {
        if !option.path.is_ident(Self::TARGET_OS) {
            return None;
        }
        Some(Self::text(&option.value).is_some_and(|target_os| target_os == self.target_os))
    }

    fn text(value: &Expr) -> Option<String> {
        match value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(text),
                ..
            }) => Some(text.value()),
            _ => None,
        }
    }

    fn holds_combinator(&self, combinator: &MetaList) -> Option<bool> {
        let operands = combinator
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .ok()?;
        let outcomes: Vec<Option<bool>> =
            operands.iter().map(|operand| self.holds(operand)).collect();
        match combinator
            .path
            .get_ident()
            .map(ToString::to_string)
            .as_deref()
        {
            Some("any") => Self::any(&outcomes),
            Some("all") => Self::negated(Self::any(&Self::each_negated(&outcomes))),
            Some("not") => Self::negated(outcomes.first().copied().flatten()),
            _ => None,
        }
    }

    fn any(outcomes: &[Option<bool>]) -> Option<bool> {
        if outcomes.contains(&Some(true)) {
            return Some(true);
        }
        if outcomes.contains(&None) {
            return None;
        }
        Some(false)
    }

    fn each_negated(outcomes: &[Option<bool>]) -> Vec<Option<bool>> {
        outcomes.iter().copied().map(Self::negated).collect()
    }

    fn negated(outcome: Option<bool>) -> Option<bool> {
        outcome.map(|holds| !holds)
    }
}

#[cfg(test)]
mod tests {
    use super::HostConfiguration;

    struct Linux;

    impl Linux {
        fn compiles(attribute: &str) -> bool {
            let item: syn::ItemFn =
                syn::parse_str(&format!("{attribute} fn run() {{}}")).expect("the item parses");
            HostConfiguration::on("linux").compiles(&item.attrs)
        }
    }

    #[test]
    fn an_item_without_cfg_compiles() {
        assert!(Linux::compiles("#[inline]"));
    }

    #[test]
    fn test_only_items_never_count_as_production() {
        assert!(!Linux::compiles("#[cfg(test)]"));
    }

    #[test]
    fn target_os_matches_the_host() {
        assert!(Linux::compiles(r#"#[cfg(target_os = "linux")]"#));
        assert!(!Linux::compiles(r#"#[cfg(target_os = "macos")]"#));
    }

    #[test]
    fn any_holds_when_one_operand_holds() {
        assert!(Linux::compiles(r#"#[cfg(any(target_os = "linux", test))]"#));
        assert!(!Linux::compiles(
            r#"#[cfg(any(target_os = "macos", test))]"#
        ));
    }

    #[test]
    fn all_holds_when_every_operand_holds() {
        assert!(!Linux::compiles(
            r#"#[cfg(all(target_os = "linux", test))]"#
        ));
    }

    #[test]
    fn not_inverts_its_operand() {
        assert!(Linux::compiles("#[cfg(not(test))]"));
        assert!(!Linux::compiles(
            r#"#[cfg(not(any(target_os = "linux", target_os = "macos")))]"#
        ));
    }

    #[test]
    fn an_unknown_predicate_counts_as_compiled() {
        assert!(Linux::compiles(r#"#[cfg(feature = "extra")]"#));
        assert!(Linux::compiles("#[cfg(unix)]"));
    }

    #[test]
    fn an_unknown_predicate_stays_compiled_under_not() {
        assert!(Linux::compiles(r#"#[cfg(not(feature = "extra"))]"#));
        assert!(Linux::compiles("#[cfg(not(windows))]"));
    }

    #[test]
    fn a_known_operand_decides_any_and_all_beside_an_unknown_one() {
        assert!(Linux::compiles(
            r#"#[cfg(any(windows, target_os = "linux"))]"#
        ));
        assert!(!Linux::compiles(r#"#[cfg(all(windows, test))]"#));
    }
}
