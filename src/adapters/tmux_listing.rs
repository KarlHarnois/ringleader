use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::tmux_output::TmuxOutput;
use crate::domain::{
    AgentPane, RepoName, SessionMetadata, SessionObservation, SessionRepo, SessionWorktree,
    TmuxAddress,
};

pub struct TmuxListing<'a> {
    output: &'a TmuxOutput,
}

impl<'a> TmuxListing<'a> {
    pub const PANE_FORMAT: &'static str = "ringleader-pane\t#{session_name}\t#{socket_path}\t#{session_created}\t#{window_active_clients}\t#{pane_id}\t#{@ringleader_agent}\t#{pane_dead}\t#{window_activity}\t#{@ringleader_meta}";
    pub const WINDOW_FORMAT: &'static str = "ringleader-window #{window_id} #{@ringleader_shell}";
    pub const AGENT_FORMAT: &'static str = "ringleader-agent #{pane_id} #{@ringleader_agent}";
    pub const PANE_PID_FORMAT: &'static str = "ringleader-pane-pid #{pane_dead} #{pane_pid}";
    pub const TTY_FORMAT: &'static str = "ringleader-tty #{pane_tty}";
    const TAGGED: &'static str = "1";
    const KEY_BINDING: &'static str = "bind-key";
    const DETACH: &'static str = "detach-client";

    pub fn new(output: &'a TmuxOutput) -> Self {
        Self { output }
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

    pub fn shows_terminal(&self, terminal: &OsStr) -> bool {
        self.output
            .labeled("ringleader-tty")
            .any(|tty| Path::new(tty) == Path::new(terminal))
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

#[derive(Serialize, Deserialize)]
pub struct MetadataJson {
    repo_name: String,
    repo_path: PathBuf,
    branch: String,
    base_commit: String,
    worktree_path: PathBuf,
    agent: String,
}

impl MetadataJson {
    pub fn encode(metadata: &SessionMetadata) -> String {
        let record = Self {
            repo_name: metadata.repo_name().to_string(),
            repo_path: metadata.repo_path().to_path_buf(),
            branch: metadata.branch().to_string(),
            base_commit: metadata.base_commit().to_string(),
            worktree_path: metadata.worktree_path().to_path_buf(),
            agent: metadata.agent().to_string(),
        };
        serde_json::to_string(&record).unwrap_or_default()
    }

    pub fn decode(text: &str) -> Option<SessionMetadata> {
        let record: Self = serde_json::from_str(text).ok()?;
        let repo = SessionRepo::new(RepoName::parse(&record.repo_name).ok()?, &record.repo_path);
        let worktree =
            SessionWorktree::new(&record.worktree_path, &record.branch, &record.base_commit);
        Some(SessionMetadata::new(repo, worktree, &record.agent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANES: &str = include_str!("../../tests/fixtures/tmux-list-panes.txt");
    const WINDOWS: &str = include_str!("../../tests/fixtures/tmux-list-windows-shell.txt");

    struct ListingFixture;

    impl ListingFixture {
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
    fn metadata_survives_an_encode_decode_round_trip() {
        let observation = ListingFixture::observation("ringleader-fix-auth");
        let metadata = observation.metadata().unwrap();

        let decoded = MetadataJson::decode(&MetadataJson::encode(metadata)).unwrap();

        assert_eq!(&decoded, metadata);
    }
}
