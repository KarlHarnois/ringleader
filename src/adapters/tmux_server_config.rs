use crate::domain::ReturnKey;

pub struct TmuxServerConfig {
    return_key: ReturnKey,
}

impl TmuxServerConfig {
    const SETTINGS: [&'static str; 16] = [
        "set -su terminal-features",
        "set -g prefix None",
        "set -g prefix2 None",
        "set -g status off",
        "set -g mouse on",
        "unbind-key -q -T root MouseDown3Pane",
        "unbind-key -q -T root M-MouseDown3Pane",
        "set -s escape-time 10",
        "set -g default-terminal tmux-256color",
        "set -s extended-keys always",
        "set -as terminal-features ',*:extkeys'",
        "set -as terminal-features ',*:RGB'",
        "set -s set-clipboard on",
        "set -s exit-unattached off",
        "set -g destroy-unattached off",
        "set -g detach-on-destroy on",
    ];

    pub const RETURN_KEY_OPTION: &'static str = "@ringleader_return_key";
    pub const HASH_OPTION: &'static str = "@ringleader_config";
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    pub fn new(return_key: &ReturnKey) -> Self {
        Self {
            return_key: return_key.clone(),
        }
    }

    pub fn render(&self) -> String {
        let hash_line = format!("set -g {} '{}'", Self::HASH_OPTION, self.hash());
        Self::joined(&[self.leading_lines(), vec![hash_line, self.return_binding()]].concat())
    }

    pub fn hash(&self) -> String {
        let hashed = [self.leading_lines(), vec![self.return_binding()]].concat();
        format!("{:016x}", Self::fnv1a(Self::joined(&hashed).as_bytes()))
    }

    fn leading_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Self::SETTINGS.iter().map(|line| line.to_string()).collect();
        lines.push(format!(
            "set -g {} {}",
            Self::RETURN_KEY_OPTION,
            self.return_key.tmux_spelling()
        ));
        lines
    }

    fn joined(lines: &[String]) -> String {
        lines.iter().map(|line| format!("{line}\n")).collect()
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(Self::FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(Self::FNV_PRIME)
        })
    }

    fn return_binding(&self) -> String {
        format!(
            "bind-key -T root {} detach-client",
            self.return_key.tmux_spelling()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::*;
    use crate::adapters::tmux_command::{ServerFiles, TmuxCommand};
    use crate::adapters::tmux_sequence::TmuxSequence;
    use crate::application::ports::{AgentLaunch, Multiplexer, SessionSpec};
    use crate::domain::{RepoName, SessionMetadata, SessionRepo, SessionWorktree};
    use crate::test_support::TmuxTestServer;

    const GOLDEN: &str = include_str!("../../tests/fixtures/tmux-server.conf");
    const KEY_CLASSES: [&str; 6] = ["C-q", "M-x", "F12", "~", "}", "C-}"];
    const DETACH_LIMIT: Duration = Duration::from_secs(2);

    struct ConfigFixture;

    impl ConfigFixture {
        fn render(key: &str) -> String {
            TmuxServerConfig::new(&ReturnKey::parse(key).unwrap()).render()
        }

        fn binding(key: &str) -> String {
            format!(
                "bind-key -T root {} detach-client\n",
                ReturnKey::parse(key).unwrap().tmux_spelling()
            )
        }

        fn detach_keys(server: &TmuxTestServer) -> Vec<String> {
            server
                .text(&["list-keys", "-T", "root"])
                .lines()
                .filter(|line| line.ends_with(" detach-client"))
                .filter_map(|line| line.split_whitespace().nth(3))
                .map(|key| key.trim_matches('"').trim_start_matches('\\').to_string())
                .collect()
        }

        fn created(server: &TmuxTestServer, name: &str) -> PathBuf {
            let worktree = server.directory().join(name);
            fs::create_dir_all(&worktree).unwrap();
            let metadata = SessionMetadata::new(
                SessionRepo::new(RepoName::parse("example-project").unwrap(), &worktree),
                SessionWorktree::new(&worktree, &format!("ringleader/{name}"), ""),
                "stand-in",
            );
            let launch = AgentLaunch::new(Vec::new(), &[], &worktree);
            let spec = SessionSpec::new(&format!("ringleader-{name}"), metadata, launch);
            server.multiplexer().create_session(&spec).unwrap();
            worktree
        }

        fn returns_from(server: &TmuxTestServer, target: &str) -> bool {
            let mut client = server.control_client(target);
            server.press(&client, "C-q");
            client.exits_with_notice_within(DETACH_LIMIT)
        }

        fn option(server: &TmuxTestServer, name: &str) -> String {
            server.text(&["show-options", "-g", "-q", "-v", name])
        }
    }

    #[test]
    fn the_default_render_matches_the_golden_fixture() {
        assert_eq!(ConfigFixture::render(ReturnKey::DEFAULT), GOLDEN);
    }

    #[test]
    fn a_server_started_with_the_render_has_no_prefix_and_no_status_line() {
        let server = TmuxTestServer::with_session("holder");

        let prefix = ConfigFixture::option(&server, "prefix");
        let status = ConfigFixture::option(&server, "status");

        assert_eq!((prefix.as_str(), status.as_str()), ("None", "off"));
    }

    #[test]
    fn sourcing_the_render_again_on_a_running_server_succeeds() {
        let server = TmuxTestServer::with_session("holder");

        let sourced = server.source(&ConfigFixture::render(ReturnKey::DEFAULT));

        assert!(sourced.status.success(), "{sourced:?}");
    }

    #[test]
    fn every_key_class_binds_detach_client_at_server_start() {
        for key in KEY_CLASSES {
            let server = TmuxTestServer::with_config(&ConfigFixture::render(key));
            server.start_session("holder");

            assert_eq!(ConfigFixture::detach_keys(&server), [key], "{key}");
        }
    }

    #[test]
    fn every_key_class_binds_detach_client_through_source_file() {
        let server = TmuxTestServer::with_config("");
        server.start_session("holder");

        for key in KEY_CLASSES {
            let sourced = server.source(&ConfigFixture::binding(key));

            assert!(sourced.status.success(), "{key}: {sourced:?}");
        }
        let mut bound = ConfigFixture::detach_keys(&server);
        bound.sort();
        let mut expected = KEY_CLASSES.map(str::to_string);
        expected.sort();
        assert_eq!(bound, expected);
    }

    #[test]
    fn every_printable_character_the_return_key_accepts_binds() {
        let server = TmuxTestServer::with_config("");
        server.start_session("holder");
        let accepted: Vec<String> = (b'!'..=b'~')
            .map(|byte| char::from(byte).to_string())
            .filter(|key| ReturnKey::parse(key).is_some())
            .collect();

        let bindings: String = accepted
            .iter()
            .map(|key| ConfigFixture::binding(key))
            .collect();
        let sourced = server.source(&bindings);

        assert!(sourced.status.success(), "{sourced:?}");
        assert_eq!(ConfigFixture::detach_keys(&server).len(), accepted.len());
    }

    #[test]
    fn the_return_key_detaches_a_client_from_an_agent_window() {
        let server = TmuxTestServer::new();
        ConfigFixture::created(&server, "fix-auth");

        assert!(ConfigFixture::returns_from(
            &server,
            "=ringleader-fix-auth:"
        ));
    }

    #[test]
    fn the_return_key_detaches_a_client_from_a_shell_window() {
        let server = TmuxTestServer::new();
        let worktree = ConfigFixture::created(&server, "fix-auth");
        let window = server
            .multiplexer()
            .ensure_shell_window("ringleader-fix-auth", &worktree, "sh")
            .unwrap();

        assert!(ConfigFixture::returns_from(&server, &window));
    }

    #[test]
    fn the_return_key_detaches_a_client_from_a_session_without_metadata() {
        let server = TmuxTestServer::with_session("work");

        assert!(ConfigFixture::returns_from(&server, "=work:"));
    }

    #[test]
    fn ctrl_b_reaches_the_pane() {
        let server = TmuxTestServer::new();
        server.text(&["new-session", "-d", "-s", "work", "cat", "-v"]);
        let client = server.control_client("=work:");

        server.press(&client, "C-b");
        server.press(&client, "Enter");

        assert!(server.wait_for(|| {
            server
                .text(&["capture-pane", "-p", "-t", "=work:"])
                .contains("^B")
        }));
    }

    #[test]
    fn a_config_path_with_brackets_loads() {
        let server = TmuxTestServer::new();
        let config = server.directory().join("conf[1]").join("tmux.conf");
        let files = ServerFiles::new(server.socket(), &config);
        files
            .prepare(&ConfigFixture::render(ReturnKey::DEFAULT))
            .unwrap();
        let tmux = TmuxCommand::new(std::ffi::OsStr::new("tmux"), &files, &server.environment());

        tmux.create(
            &TmuxSequence::new().command(["new-session", "-d", "-s", "holder", "sleep", "30"]),
            &[],
        )
        .unwrap();

        assert_eq!(ConfigFixture::option(&server, "prefix"), "None");
    }

    #[test]
    fn the_users_tmux_config_files_never_load() {
        let server = TmuxTestServer::new();
        let home = server.directory();
        fs::write(home.join(".tmux.conf"), "set -g @example-marker home\n").unwrap();
        let xdg = Path::new(home).join(".config").join("tmux");
        fs::create_dir_all(&xdg).unwrap();
        fs::write(xdg.join("tmux.conf"), "set -g @example-marker xdg\n").unwrap();

        ConfigFixture::created(&server, "fix-auth");

        assert_eq!(ConfigFixture::option(&server, "@example-marker"), "");
    }
}
