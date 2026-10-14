use std::ffi::OsString;
use std::path::Path;

use super::metadata_json::MetadataJson;
use super::tmux_launch::TmuxLaunch;
use super::tmux_listing::TmuxListing;
use super::tmux_sequence::TmuxSequence;
use crate::application::ports::{AgentLaunch, SessionSpec};
use crate::domain::SessionMetadata;

pub struct TmuxScripts;

impl TmuxScripts {
    const SESSION_CREATED: &'static str = "ringleader-created #{session_id} #{pane_id}";
    const WINDOW_CREATED: &'static str = "ringleader-created #{window_id} #{pane_id}";

    pub fn exact(tmux_name: &str) -> String {
        format!("={tmux_name}:")
    }

    pub fn create_session(spec: &SessionSpec, launcher: &[OsString]) -> TmuxSequence {
        let target = Self::exact(spec.tmux_name());
        let size = spec.size();
        let mut new_session = Self::words(&[
            "new-session",
            "-d",
            "-s",
            spec.tmux_name(),
            "-x",
            &size.width().to_string(),
            "-y",
            &size.height().to_string(),
        ]);
        new_session.extend(Self::start_in(spec.worktree()));
        new_session.extend(Self::words(&["-P", "-F", Self::SESSION_CREATED]));
        new_session.extend(launcher.iter().cloned());
        TmuxSequence::new()
            .command(new_session)
            .command(["set-option", "-p", "-t", &target, "@ringleader_agent", "1"])
            .command(["set-option", "-p", "-t", &target, "remain-on-exit", "on"])
            .command(["set-option", "-t", &target, "destroy-unattached", "off"])
            .command(Self::metadata_option(&target, spec.metadata()))
            .command([
                "display-message",
                "-p",
                "-t",
                &target,
                "ringleader-done #{pane_id} #{socket_path}",
            ])
    }

    pub fn agent_window(
        tmux_name: &str,
        launch: &AgentLaunch,
        launcher: &[OsString],
    ) -> TmuxSequence {
        let target = Self::exact(tmux_name);
        let mut new_window = Self::words(&["new-window", "-t", &target]);
        new_window.extend(Self::start_in(launch.worktree()));
        new_window.extend(Self::words(&["-P", "-F", Self::WINDOW_CREATED]));
        new_window.extend(launcher.iter().cloned());
        TmuxSequence::new()
            .command(new_window)
            .command(["set-option", "-p", "-t", &target, "@ringleader_agent", "1"])
            .command(["set-option", "-p", "-t", &target, "remain-on-exit", "on"])
            .command([
                "display-message",
                "-p",
                "-t",
                &target,
                "ringleader-done #{pane_id}",
            ])
    }

    pub fn respawn(pane: &str, launch: &AgentLaunch, launcher: &[OsString]) -> TmuxSequence {
        let mut respawn = Self::words(&["respawn-pane", "-t", pane]);
        respawn.extend(Self::start_in(launch.worktree()));
        respawn.extend(launcher.iter().cloned());
        TmuxSequence::new().command(respawn).command([
            "display-message",
            "-p",
            "-t",
            pane,
            TmuxLaunch::DONE_LABEL,
        ])
    }

    pub fn shell_window(tmux_name: &str, worktree: &Path, shell: &str) -> TmuxSequence {
        let target = Self::exact(tmux_name);
        let mut new_window = Self::words(&["new-window", "-t", &target]);
        new_window.extend(Self::start_in(worktree));
        new_window.extend(Self::words(&["-P", "-F", Self::WINDOW_CREATED, shell]));
        TmuxSequence::new()
            .command(new_window)
            .command(["set-option", "-w", "-t", &target, "@ringleader_shell", "1"])
            .command([
                "display-message",
                "-p",
                "-t",
                &target,
                "ringleader-done #{window_id}",
            ])
    }

    pub fn shell_windows(tmux_name: &str) -> TmuxSequence {
        TmuxSequence::new().command([
            "list-windows",
            "-t",
            &Self::exact(tmux_name),
            "-F",
            TmuxListing::WINDOW_FORMAT,
        ])
    }

    pub fn rename(old: &str, new: &str, metadata: &SessionMetadata) -> TmuxSequence {
        let renamed = Self::exact(new);
        TmuxSequence::new()
            .command(["rename-session", "-t", &Self::exact(old), new])
            .command(Self::metadata_option(&renamed, metadata))
            .command([
                "display-message",
                "-p",
                "-t",
                &renamed,
                TmuxLaunch::DONE_LABEL,
            ])
    }

    pub fn kill(tmux_name: &str) -> TmuxSequence {
        TmuxSequence::new()
            .command(["kill-session", "-t", &Self::exact(tmux_name)])
            .command(["display-message", "-p", TmuxLaunch::DONE_LABEL])
    }

    pub fn session_panes(tmux_name: &str, format: &str) -> TmuxSequence {
        TmuxSequence::new().command([
            "list-panes",
            "-s",
            "-t",
            &Self::exact(tmux_name),
            "-F",
            format,
        ])
    }

    pub fn all_panes(format: &str) -> TmuxSequence {
        TmuxSequence::new().command(["list-panes", "-a", "-F", format])
    }

    pub fn pane_value(pane: &str, format: &str) -> TmuxSequence {
        TmuxSequence::new().command(["display-message", "-p", "-t", pane, format])
    }

    pub fn server_state() -> TmuxSequence {
        Self::server_values(&[TmuxListing::SERVER_FORMAT]).command([
            "list-sessions",
            "-F",
            TmuxListing::SESSION_FORMAT,
        ])
    }

    pub fn server_values(formats: &[impl AsRef<str>]) -> TmuxSequence {
        formats
            .iter()
            .fold(TmuxSequence::new(), |sequence, format| {
                sequence.command(["display-message", "-p", format.as_ref()])
            })
    }

    pub fn type_text(pane: &str, text: &str) -> TmuxSequence {
        TmuxSequence::new()
            .command(["send-keys", "-t", pane, "-l", "--", text])
            .command(["display-message", "-p", "-t", pane, TmuxLaunch::DONE_LABEL])
    }

    pub fn press_enter(pane: &str) -> TmuxSequence {
        TmuxSequence::new()
            .command(["send-keys", "-t", pane, "Enter"])
            .command(["display-message", "-p", "-t", pane, TmuxLaunch::DONE_LABEL])
    }

    pub fn capture_row(pane: &str, row: usize) -> TmuxSequence {
        let row = row.to_string();
        TmuxSequence::new().command(["capture-pane", "-p", "-t", pane, "-S", &row, "-E", &row])
    }

    pub fn capture(pane: &str) -> TmuxSequence {
        TmuxSequence::new().command(["capture-pane", "-e", "-p", "-t", pane])
    }

    pub fn root_keys() -> TmuxSequence {
        TmuxSequence::new().command(["list-keys", "-T", "root"])
    }

    pub fn attach_session(target: &str) -> TmuxSequence {
        TmuxSequence::new().command(["attach-session", "-t", target])
    }

    fn metadata_option(target: &str, metadata: &SessionMetadata) -> Vec<OsString> {
        let encoded = MetadataJson::encode(metadata);
        Self::words(&["set-option", "-t", target, "@ringleader_meta", &encoded])
    }

    fn start_in(directory: &Path) -> Vec<OsString> {
        vec![
            OsString::from("-c"),
            TmuxSequence::start_directory(directory),
        ]
    }

    fn words(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }
}
