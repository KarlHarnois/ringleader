use std::ffi::{OsStr, OsString};
use std::iter::once;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPopups {
    finder_prefix: Vec<OsString>,
    assignments: Vec<OsString>,
}

impl TmuxPopups {
    const POPUP: &'static str = "display-popup";
    const KEEP_OPEN_ON_FAILURE: &'static str = "-EE";
    const BORDERLESS: &'static str = "-B";
    const ASSIGNMENT_FLAG: &'static str = "-e";
    const CLIENT_NAME: &'static str = "#{client_name}";
    const FORMAT_MARK: &'static str = "#";
    const ESCAPED_FORMAT_MARK: &'static str = "##";
    const QUOTE: &'static str = "'";
    const ESCAPED_QUOTE: &'static str = r"'\''";
    const DOUBLE_QUOTED_SPECIALS: [char; 3] = ['\\', '"', '$'];

    pub fn new(finder_prefix: Vec<OsString>, assignments: &[OsString]) -> Self {
        Self {
            finder_prefix,
            assignments: assignments.to_vec(),
        }
    }

    pub fn finder_command(&self) -> String {
        format!("run-shell -C \"{}\"", Self::double_quoted(&self.popup()))
    }

    fn popup(&self) -> String {
        let flags = [Self::POPUP, Self::KEEP_OPEN_ON_FAILURE, Self::BORDERLESS].map(String::from);
        let assignments = self.assignments.iter().flat_map(|assignment| {
            [
                Self::ASSIGNMENT_FLAG.to_string(),
                Self::verbatim(assignment),
            ]
        });
        let finder = self.finder_prefix.iter().map(|word| Self::verbatim(word));
        let client = Self::single_quoted(Self::CLIENT_NAME);
        let words: Vec<String> = flags
            .into_iter()
            .chain(assignments)
            .chain(finder)
            .chain(once(client))
            .collect();
        words.join(" ")
    }

    fn verbatim(word: &OsStr) -> String {
        let unexpanded = word
            .to_string_lossy()
            .replace(Self::FORMAT_MARK, Self::ESCAPED_FORMAT_MARK);
        Self::single_quoted(&unexpanded)
    }

    fn single_quoted(word: &str) -> String {
        format!("'{}'", word.replace(Self::QUOTE, Self::ESCAPED_QUOTE))
    }

    fn double_quoted(text: &str) -> String {
        text.chars()
            .flat_map(
                |character| match Self::DOUBLE_QUOTED_SPECIALS.contains(&character) {
                    true => vec!['\\', character],
                    false => vec![character],
                },
            )
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::adapters::tmux::tmux_server_config::TmuxServerConfig;
    use crate::domain::KeyBindings;
    use crate::test_support::tmux_test_server::ControlClient;
    use crate::test_support::{ExecutableScript, TmuxTestServer};

    struct PopupFixture {
        server: TmuxTestServer,
        finder: PathBuf,
    }

    impl PopupFixture {
        const FINDER_KEY: &'static str = "C-k";
        const LINES_PER_RUN: usize = 3;
        const RECORDING_FINDER: &'static str = "#!/bin/sh\n\
            printf '%s\\n' \"$@\" \"$RINGLEADER_TMUX\" \"$RINGLEADER_GIT\" >> \"$0.runs\"\n\
            exit \"${FINDER_EXIT:-0}\"\n";
        const ASSIGNMENTS: [&'static str; 2] = [
            "RINGLEADER_TMUX=/nix/store/example-tmux/bin/tmux",
            "RINGLEADER_GIT=/nix/store/example-git/bin/git",
        ];

        fn new() -> Self {
            Self::with_finder_in("finder")
        }

        fn with_finder_in(directory: &str) -> Self {
            let server = TmuxTestServer::with_session("holder");
            let finder = server.directory().join(directory).join("finder");
            fs::create_dir_all(finder.parent().unwrap()).unwrap();
            ExecutableScript::write(&finder, Self::RECORDING_FINDER);
            let sourced = server.source(&Self::render(&Self::popup(&finder, &[])));
            assert!(sourced.status.success(), "{sourced:?}");
            Self { server, finder }
        }

        fn failing() -> Self {
            let fixture = Self::new();
            fixture
                .server
                .text(&["set-environment", "-g", "FINDER_EXIT", "1"]);
            fixture
        }

        fn popup(finder: &Path, extra: &[&str]) -> TmuxPopups {
            let prefix = once(finder.as_os_str())
                .chain(extra.iter().map(OsStr::new))
                .map(OsStr::to_os_string)
                .collect();
            TmuxPopups::new(prefix, &Self::ASSIGNMENTS.map(OsString::from))
        }

        fn render(popup: &TmuxPopups) -> String {
            TmuxServerConfig::new(&KeyBindings::default(), popup).render()
        }

        fn client(&self) -> ControlClient {
            self.server.control_client("=holder:")
        }

        fn press_finder_key(&self, client: &ControlClient) {
            self.server.press(client, Self::FINDER_KEY);
        }

        fn close_popup(&self, client: &ControlClient) {
            let name = self.server.client_name(client);
            self.server.text(&["display-popup", "-C", "-c", &name]);
        }

        fn runs_reach(&self, count: usize) -> bool {
            self.server.wait_for(|| self.runs().len() == count)
        }

        fn runs(&self) -> Vec<Vec<String>> {
            let text = fs::read_to_string(self.recording()).unwrap_or_default();
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            lines
                .chunks(Self::LINES_PER_RUN)
                .map(<[String]>::to_vec)
                .collect()
        }

        fn recording(&self) -> PathBuf {
            let mut path = self.finder.clone().into_os_string();
            path.push(".runs");
            PathBuf::from(path)
        }
    }

    #[test]
    fn the_command_opens_a_borderless_popup_that_names_the_client_and_the_programs() {
        let popup = PopupFixture::popup(Path::new("/bin/ringleader"), &["__find-session"]);

        assert_eq!(
            popup.finder_command(),
            "run-shell -C \"display-popup -EE -B \
             -e 'RINGLEADER_TMUX=/nix/store/example-tmux/bin/tmux' \
             -e 'RINGLEADER_GIT=/nix/store/example-git/bin/git' \
             '/bin/ringleader' '__find-session' '#{client_name}'\""
        );
    }

    #[test]
    fn the_finder_key_runs_the_finder_with_the_pressing_client_and_the_programs() {
        let fixture = PopupFixture::new();
        let client = fixture.client();

        fixture.press_finder_key(&client);

        assert!(fixture.runs_reach(1), "{:?}", fixture.runs());
        assert_eq!(
            fixture.runs()[0],
            [
                fixture.server.client_name(&client),
                "/nix/store/example-tmux/bin/tmux".to_string(),
                "/nix/store/example-git/bin/git".to_string(),
            ]
        );
    }

    #[test]
    fn the_finder_key_opens_the_finder_from_either_copy_mode() {
        for mode_keys in ["emacs", "vi"] {
            let fixture = PopupFixture::new();
            fixture
                .server
                .text(&["set-option", "-g", "mode-keys", mode_keys]);
            fixture.server.text(&["copy-mode", "-t", "=holder:"]);
            let client = fixture.client();

            fixture.press_finder_key(&client);

            assert!(fixture.runs_reach(1), "{mode_keys}");
        }
    }

    #[test]
    fn a_finder_path_with_quotes_dollars_and_format_marks_runs_as_written() {
        let fixture = PopupFixture::with_finder_in("it's a \"#{dir}\" $HOME \\ dir");
        let client = fixture.client();

        fixture.press_finder_key(&client);

        assert!(fixture.runs_reach(1));
    }

    #[test]
    fn a_finished_finder_closes_its_popup() {
        let fixture = PopupFixture::new();
        let client = fixture.client();
        fixture.press_finder_key(&client);
        assert!(fixture.runs_reach(1));

        fixture.press_finder_key(&client);

        assert!(fixture.runs_reach(2));
    }

    #[test]
    fn a_failed_finder_keeps_its_popup_open_to_show_why() {
        let fixture = PopupFixture::failing();
        let client = fixture.client();
        fixture.press_finder_key(&client);
        assert!(fixture.runs_reach(1));

        fixture.press_finder_key(&client);
        let reopened_while_open = fixture.runs_reach(2);
        fixture.close_popup(&client);
        fixture.press_finder_key(&client);

        assert!(!reopened_while_open);
        assert!(fixture.runs_reach(2));
    }
}
