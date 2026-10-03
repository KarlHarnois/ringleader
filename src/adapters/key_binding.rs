#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    line: String,
    repeat: bool,
    table: String,
    key: String,
    command: String,
}

impl KeyBinding {
    const RETURN_TABLE: &'static str = "root";
    const RETURN_KEY: &'static str = "C-q";
    const SWITCH_PREFIX: &'static str = "\"switch-client -t ";
    const FORWARD: &'static str = "\"send-keys C-q\"";

    pub fn find_return_key(listing: &str) -> Option<Self> {
        listing
            .lines()
            .filter_map(Self::parse)
            .find(|binding| binding.table == Self::RETURN_TABLE && binding.key == Self::RETURN_KEY)
    }

    pub fn return_command(prefix: &str, return_branch: &str) -> String {
        let listed_branch = if return_branch.contains(' ') {
            format!("\"{return_branch}\"")
        } else {
            return_branch.to_string()
        };
        format!(
            "if-shell -F \"#{{m:{prefix}*,#{{session_name}}}}\" {listed_branch} {}",
            Self::FORWARD
        )
    }

    pub fn condition(prefix: &str) -> String {
        format!("#{{m:{prefix}*,#{{session_name}}}}")
    }

    pub fn line(&self) -> &str {
        &self.line
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn same_fields_as(&self, other: &KeyBinding) -> bool {
        self.repeat == other.repeat
            && self.table == other.table
            && self.key == other.key
            && self.command == other.command
    }

    pub fn is_ringleaders(&self) -> bool {
        let Some(rest) = self.command.strip_prefix("if-shell -F \"#{m:") else {
            return false;
        };
        let Some((prefix, branches)) = rest.split_once("*,#{session_name}}\" ") else {
            return false;
        };
        Self::is_prefix(prefix) && Self::is_return_branches(branches)
    }

    fn parse(line: &str) -> Option<Self> {
        let mut tokens = Tokens::new(line);
        if tokens.next()? != "bind-key" {
            return None;
        }
        let mut flag = tokens.next()?;
        let repeat = flag == "-r";
        if repeat {
            flag = tokens.next()?;
        }
        if flag != "-T" {
            return None;
        }
        let table = tokens.next()?.to_string();
        let key = tokens.next()?.to_string();
        Some(Self {
            line: line.to_string(),
            repeat,
            table,
            key,
            command: tokens.rest().to_string(),
        })
    }

    fn is_prefix(prefix: &str) -> bool {
        let body = prefix.strip_suffix('-').unwrap_or("");
        !body.is_empty()
            && prefix.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
    }

    fn is_return_branches(branches: &str) -> bool {
        let Some(return_branch) = branches.strip_suffix(Self::FORWARD) else {
            return false;
        };
        let return_branch = return_branch.trim_end();
        if return_branch == "detach-client" {
            return true;
        }
        return_branch
            .strip_prefix(Self::SWITCH_PREFIX)
            .and_then(|target| target.strip_suffix('"'))
            .is_some_and(|target| !target.is_empty() && !target.contains('"'))
    }
}

struct Tokens<'a> {
    line: &'a str,
    position: usize,
}

impl<'a> Tokens<'a> {
    fn new(line: &'a str) -> Self {
        Self { line, position: 0 }
    }

    fn rest(&self) -> &'a str {
        self.line[self.position..].trim_start()
    }

    fn next(&mut self) -> Option<&'a str> {
        let rest = &self.line[self.position..];
        let start = self.position + (rest.len() - rest.trim_start().len());
        let token_length = self.line[start..]
            .find(char::is_whitespace)
            .unwrap_or(self.line.len() - start);
        if token_length == 0 {
            return None;
        }
        self.position = start + token_length;
        Some(&self.line[start..self.position])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINDINGS: &str = include_str!("../../tests/fixtures/tmux-bindings.txt");

    struct BindingFixture;

    impl BindingFixture {
        fn listing(name: &str) -> String {
            let marker = format!("### {name}\n");
            let start = BINDINGS.find(&marker).unwrap() + marker.len();
            let end = BINDINGS[start..]
                .find("### ")
                .map_or(BINDINGS.len(), |end| start + end);
            BINDINGS[start..end].to_string()
        }

        fn return_key(name: &str) -> Option<KeyBinding> {
            KeyBinding::find_return_key(&Self::listing(name))
        }
    }

    #[test]
    fn a_user_binding_is_found_in_a_padded_listing() {
        for version in ["3.6", "3.7"] {
            let binding = BindingFixture::return_key(&format!("{version} user binding")).unwrap();

            assert_eq!(binding.command(), "send-keys -X cancel");
        }
    }

    #[test]
    fn a_repeatable_binding_keeps_its_flag() {
        let binding = BindingFixture::return_key("3.6 repeat binding").unwrap();

        assert!(binding.line().starts_with("bind-key -r"));
        assert_eq!(binding.command(), "next-window");
    }

    #[test]
    fn no_return_key_line_means_unbound() {
        assert_eq!(BindingFixture::return_key("3.7 no binding"), None);
    }

    #[test]
    fn a_binding_in_another_table_is_not_the_return_key() {
        assert_eq!(BindingFixture::return_key("3.6 other tables only"), None);
    }

    #[test]
    fn a_brace_binding_is_read_as_3_6_lists_it() {
        let binding = BindingFixture::return_key("3.6 brace binding").unwrap();

        assert_eq!(
            binding.command(),
            "display-message one \\;\\; display-message two"
        );
    }

    #[test]
    fn padding_widths_never_change_the_fields() {
        let narrow = BindingFixture::return_key("3.6 narrow table").unwrap();
        let wide = BindingFixture::return_key("3.6 wide table").unwrap();

        assert_ne!(narrow.line(), wide.line());
        assert!(narrow.same_fields_as(&wide));
    }

    #[test]
    fn both_return_shapes_are_recognized_for_any_prefix() {
        let detach = KeyBinding::return_command("old-prefix-", "detach-client");
        let switch = KeyBinding::return_command("rl-", "switch-client -t %12");

        for command in [detach, switch] {
            let line = format!("bind-key -T root C-q {command}");
            assert!(KeyBinding::find_return_key(&line).unwrap().is_ringleaders());
        }
    }

    #[test]
    fn a_user_if_shell_is_not_ringleaders() {
        let line = "bind-key -T root C-q if-shell -F \"#{m:work*,#{session_name}}\" detach-client \"send-keys C-q\"";

        assert!(!KeyBinding::find_return_key(line).unwrap().is_ringleaders());
    }
}
