use std::ffi::OsStr;
use std::path::Path;

use super::metadata_json::MetadataJson;
use super::tmux_output::TmuxOutput;
use crate::application::ports::ServerState;
use crate::domain::{AgentPane, SessionObservation, TmuxAddress};

pub struct TmuxListing<'a> {
    output: &'a TmuxOutput,
}

impl<'a> TmuxListing<'a> {
    pub const AGENT_TAG: &'static str = "@ringleader_agent";
    pub const SHELL_TAG: &'static str = "@ringleader_shell";
    pub const METADATA_OPTION: &'static str = "@ringleader_meta";
    pub const PANE_PID_FORMAT: &'static str = "ringleader-pane-pid #{pane_dead} #{pane_pid}";
    pub const TTY_FORMAT: &'static str = "ringleader-tty #{pane_dead} #{pane_tty}";
    pub const WINDOW_SIZE_FORMAT: &'static str =
        "ringleader-window-size #{window_width}x#{window_height}";
    pub const SERVER_FORMAT: &'static str = "ringleader-server #{version}";
    pub const SERVER_PID_FORMAT: &'static str = "ringleader-server-pid #{pid}";
    pub const SESSION_FORMAT: &'static str = "ringleader-session";
    pub const SESSION_ID_FORMAT: &'static str =
        "ringleader-session-id #{session_id} #{session_name}";
    const TAGGED: &'static str = "1";
    const DEAD: &'static str = "1";
    const KEY_BINDING: &'static str = "bind-key";
    const DETACH: &'static str = "detach-client";

    pub fn new(output: &'a TmuxOutput) -> Self {
        Self { output }
    }

    pub fn pane_format() -> String {
        format!(
            "ringleader-pane\t#{{session_name}}\t#{{socket_path}}\t#{{session_created}}\t#{{window_active_clients}}\t#{{pane_id}}\t#{{{agent}}}\t#{{pane_dead}}\t#{{window_activity}}\t#{{{metadata}}}",
            agent = Self::AGENT_TAG,
            metadata = Self::METADATA_OPTION
        )
    }

    pub fn agent_format() -> String {
        format!("ringleader-agent #{{pane_id}} #{{{}}}", Self::AGENT_TAG)
    }

    pub fn window_format() -> String {
        format!("ringleader-window #{{window_id}} #{{{}}}", Self::SHELL_TAG)
    }

    pub fn observations(&self) -> Vec<SessionObservation> {
        let rows: Vec<PaneRow> = self
            .output
            .labeled("ringleader-pane")
            .filter_map(PaneRow::parse)
            .collect();
        let mut sessions: Vec<&str> = Vec::new();
        for row in &rows {
            if !sessions.contains(&row.session) {
                sessions.push(row.session);
            }
        }
        sessions
            .iter()
            .map(|session| Self::observe_session(&rows, session))
            .collect()
    }

    pub fn shell_window(&self) -> Option<String> {
        self.output.labeled("ringleader-window").find_map(|row| {
            let (window, tag) = row.split_once(' ')?;
            (tag == Self::TAGGED).then(|| window.to_string())
        })
    }

    pub fn server_state(&self) -> Option<ServerState> {
        let version = self.output.first("ringleader-server")?;
        let sessions = self.output.labeled(Self::SESSION_FORMAT).count();
        Some(ServerState::new(version, sessions))
    }

    pub fn server_pid(&self) -> Option<u32> {
        self.output.first("ringleader-server-pid")?.parse().ok()
    }

    pub fn window_resized(&self) -> bool {
        let mut sizes = self.output.labeled("ringleader-window-size");
        sizes.next() != sizes.next()
    }

    pub fn shows_terminal(&self, terminal: &OsStr) -> bool {
        self.output
            .labeled("ringleader-tty")
            .filter_map(|row| row.split_once(' '))
            .filter(|(dead, _)| *dead != Self::DEAD)
            .any(|(_, tty)| Path::new(tty) == Path::new(terminal))
    }

    pub fn binds_detach(&self) -> bool {
        self.output.stdout().lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            words.first() == Some(&Self::KEY_BINDING) && words.last() == Some(&Self::DETACH)
        })
    }

    pub fn agent_pane(&self) -> Option<String> {
        self.output.labeled("ringleader-agent").find_map(|row| {
            let (pane, tag) = row.split_once(' ')?;
            (tag == Self::TAGGED).then(|| pane.to_string())
        })
    }

    pub fn session_id_named(&self, tmux_name: &str) -> Option<String> {
        self.output
            .labeled("ringleader-session-id")
            .find_map(|row| {
                let (id, name) = row.split_once(' ')?;
                (name == tmux_name).then(|| id.to_string())
            })
    }

    pub fn live_pane_pids(&self) -> Vec<u32> {
        self.output
            .labeled("ringleader-pane-pid")
            .filter_map(|row| row.split_once(' '))
            .filter(|(dead, _)| *dead == "0")
            .filter_map(|(_, pid)| pid.parse().ok())
            .collect()
    }

    fn observe_session(rows: &[PaneRow], session: &str) -> SessionObservation {
        let session_rows: Vec<&PaneRow> =
            rows.iter().filter(|row| row.session == session).collect();
        let tagged = session_rows
            .iter()
            .find(|row| row.agent_tag == Self::TAGGED);
        let first = session_rows[0];
        let tmux = TmuxAddress::new(session, Path::new(first.socket));
        let observation = SessionObservation::new(tmux, first.created_at);
        let observation = match tagged {
            Some(row) => observation.with_agent_pane(row.agent_pane()),
            None => observation,
        };
        match MetadataJson::decode(first.metadata) {
            Some(metadata) => observation.with_metadata(metadata),
            None => observation,
        }
    }
}

struct PaneRow<'a> {
    session: &'a str,
    socket: &'a str,
    created_at: u64,
    active_clients: u32,
    agent_tag: &'a str,
    dead: bool,
    activity_at: u64,
    metadata: &'a str,
}

impl<'a> PaneRow<'a> {
    const COLUMNS: usize = 9;

    fn parse(row: &'a str) -> Option<Self> {
        let columns: Vec<&str> = row.splitn(Self::COLUMNS, '\t').collect();
        let [
            session,
            socket,
            created,
            clients,
            _pane,
            tag,
            dead,
            activity,
            metadata,
        ] = columns.as_slice()
        else {
            return None;
        };
        Some(Self {
            session,
            socket,
            created_at: created.parse().unwrap_or(0),
            active_clients: clients.parse().unwrap_or(0),
            agent_tag: tag,
            dead: *dead == "1",
            activity_at: activity.parse().unwrap_or(0),
            metadata,
        })
    }

    fn agent_pane(&self) -> AgentPane {
        let pane = if self.dead {
            AgentPane::dead(self.activity_at)
        } else {
            AgentPane::live(self.activity_at)
        };
        if self.active_clients > 0 {
            pane.shown_by_a_client()
        } else {
            pane
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANES: &str = include_str!("../../../tests/fixtures/tmux-list-panes.txt");
    const WINDOWS: &str = include_str!("../../../tests/fixtures/tmux-list-windows-shell.txt");

    struct ListingFixture;

    impl ListingFixture {
        const SIZE_PLACEHOLDER: &str = "#{window_width}x#{window_height}";

        fn observations() -> Vec<SessionObservation> {
            let output = TmuxOutput::new(true, PANES, "");
            TmuxListing::new(&output).observations()
        }

        fn observation(name: &str) -> SessionObservation {
            Self::observations()
                .into_iter()
                .find(|observation| observation.tmux_name() == name)
                .unwrap()
        }

        fn window_sizes(before: &str, after: &str) -> TmuxOutput {
            let stdout = [before, after]
                .map(|size| TmuxListing::WINDOW_SIZE_FORMAT.replace(Self::SIZE_PLACEHOLDER, size))
                .join("\n");
            TmuxOutput::new(true, &stdout, "")
        }
    }

    #[test]
    fn one_observation_per_session_in_listing_order() {
        let names: Vec<String> = ListingFixture::observations()
            .iter()
            .map(|observation| observation.tmux_name().to_string())
            .collect();

        assert_eq!(
            names,
            [
                "ringleader-fix-auth",
                "ringleader-dark-mode",
                "ringleader-docs",
                "ringleader-mystery",
                "ringleader-lost-pane"
            ]
        );
    }

    #[test]
    fn a_dead_agent_pane_beside_a_live_shell_pane_reads_as_dead() {
        assert!(ListingFixture::observation("ringleader-docs").pane_dead());
    }

    #[test]
    fn only_the_tagged_pane_of_a_split_agent_window_is_the_agent() {
        let observation = ListingFixture::observation("ringleader-fix-auth");

        assert!(!observation.pane_dead());
        assert_eq!(observation.activity_at(), 1_791_000_100);
    }

    #[test]
    fn active_clients_on_the_agent_window_mark_it_attached() {
        assert!(ListingFixture::observation("ringleader-dark-mode").attached());
        assert!(!ListingFixture::observation("ringleader-fix-auth").attached());
    }

    #[test]
    fn the_socket_path_and_creation_time_come_from_the_row() {
        let observation = ListingFixture::observation("ringleader-fix-auth");

        assert_eq!(
            observation.socket_path(),
            Path::new("/tmp/tmux-1000/default")
        );
        assert_eq!(observation.created_at(), 1_791_000_000);
    }

    #[test]
    fn valid_metadata_is_decoded() {
        let observation = ListingFixture::observation("ringleader-fix-auth");

        assert_eq!(
            observation.metadata().unwrap().branch(),
            "ringleader/fix-auth"
        );
    }

    #[test]
    fn malformed_metadata_reads_as_none() {
        assert!(
            ListingFixture::observation("ringleader-mystery")
                .metadata()
                .is_none()
        );
    }

    #[test]
    fn a_session_without_a_tagged_pane_keeps_its_metadata_and_reads_as_dead() {
        let observation = ListingFixture::observation("ringleader-lost-pane");

        assert!(observation.metadata().is_some());
        assert!(observation.pane_dead());
    }

    #[test]
    fn the_shell_window_is_found_by_its_tag_even_after_a_rename() {
        let output = TmuxOutput::new(true, WINDOWS, "");

        assert_eq!(
            TmuxListing::new(&output).shell_window(),
            Some("@7".to_string())
        );
    }

    #[test]
    fn a_window_whose_size_changed_reads_as_resized() {
        let output = ListingFixture::window_sizes("170x50", "46x22");

        assert!(TmuxListing::new(&output).window_resized());
    }

    #[test]
    fn a_window_that_kept_its_size_reads_as_not_resized() {
        let output = ListingFixture::window_sizes("46x22", "46x22");

        assert!(!TmuxListing::new(&output).window_resized());
    }
}
