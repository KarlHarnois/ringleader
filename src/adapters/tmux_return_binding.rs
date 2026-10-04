use std::path::Path;

use super::binding_records::{BindingRecord, BindingRecords, ServerIdentity};
use super::key_binding::KeyBinding;
use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;
use crate::application::ports::{BindingError, ReturnBinding};

pub struct TmuxReturnBinding {
    tmux: TmuxCommand,
    records: BindingRecords,
    target: ReturnTarget,
    tmux_prefix: String,
}

impl TmuxReturnBinding {
    const DONE: &'static str = "ringleader-done";

    pub fn new(tmux: TmuxCommand, records: &Path, inside_tmux: bool, tmux_prefix: &str) -> Self {
        let target = match (inside_tmux, tmux.caller_pane()) {
            (false, _) => ReturnTarget::Detach,
            (true, Some(pane)) => ReturnTarget::Pane(pane),
            (true, None) => ReturnTarget::Unavailable,
        };
        Self {
            tmux,
            records: BindingRecords::new(records),
            target,
            tmux_prefix: tmux_prefix.to_string(),
        }
    }

    fn return_branch(&self) -> Result<String, BindingError> {
        match &self.target {
            ReturnTarget::Detach => Ok("detach-client".to_string()),
            ReturnTarget::Pane(pane) => {
                self.confirm_pane(pane)?;
                Ok(format!("switch-client -t {pane}"))
            }
            ReturnTarget::Unavailable => Err(BindingError::Unavailable {
                reason: "TMUX_PANE is not set, as in a display-popup".to_string(),
            }),
        }
    }

    fn confirm_pane(&self, pane: &str) -> Result<(), BindingError> {
        let lookup = TmuxSequence::new().command([
            "display-message",
            "-p",
            "-t",
            pane,
            "ringleader-pane #{pane_id}",
        ]);
        let output = self.tmux.query(&lookup)?;
        if output.first("ringleader-pane") == Some(pane) {
            return Ok(());
        }
        Err(BindingError::Unavailable {
            reason: format!("TMUX_PANE {pane} no longer names a pane"),
        })
    }

    fn server_identity(tmux: &TmuxCommand) -> Result<Option<ServerIdentity>, TmuxError> {
        let query = TmuxSequence::new().command([
            "display-message",
            "-p",
            "ringleader-server #{pid} #{socket_path}",
        ]);
        let output = tmux.query(&query)?;
        if output.reports_no_server() {
            return Ok(None);
        }
        let values = output
            .first("ringleader-server")
            .ok_or_else(|| TmuxError::failed("display-message", &output))?;
        let (pid, socket) = values.split_once(' ').unwrap_or((values, ""));
        Ok(Some(ServerIdentity::new(pid, Path::new(socket))))
    }

    fn return_key(tmux: &TmuxCommand) -> Result<Option<KeyBinding>, TmuxError> {
        let output = tmux.query(&TmuxSequence::new().command(["list-keys"]))?;
        let listing = output.stdout();
        if !output.succeeded() && !listing.lines().any(|line| line.starts_with("bind-key")) {
            return Err(TmuxError::failed("list-keys", &output));
        }
        Ok(KeyBinding::find_return_key(listing))
    }

    fn bind(&self, return_branch: &str) -> Result<(), BindingError> {
        let bind = TmuxSequence::new()
            .command([
                "bind-key",
                "-T",
                "root",
                "C-q",
                "if-shell",
                "-F",
                &KeyBinding::condition(&self.tmux_prefix),
                return_branch,
                "send-keys C-q",
            ])
            .command(["display-message", "-p", Self::DONE]);
        Self::require_done("bind-key", &self.tmux.change(&bind)?)
    }

    fn restore_on(&self, tmux: &TmuxCommand, record: &BindingRecord) -> Result<(), BindingError> {
        let current = Self::return_key(tmux)?;
        if !current.is_some_and(|binding| binding.is_ringleaders()) {
            return Ok(());
        }
        let Some(saved_line) = record.saved() else {
            return Self::unbind(tmux);
        };
        let restored = Self::replay(tmux, saved_line).and_then(|()| Self::verify(tmux, saved_line));
        match restored {
            Err(error) if error.is_timeout() => Err(error),
            Err(_) => {
                let _ = Self::unbind(tmux);
                log::warn!("could not restore the saved Ctrl-Q binding: {saved_line}");
                Err(BindingError::NotRestored {
                    saved_line: saved_line.to_string(),
                })
            }
            Ok(()) => Ok(()),
        }
    }

    fn replay(tmux: &TmuxCommand, saved_line: &str) -> Result<(), BindingError> {
        let source = TmuxSequence::new().command(["source-file", "-"]);
        let output = tmux.change_with_input(&source, format!("{saved_line}\n").as_bytes())?;
        if output.succeeded() {
            return Ok(());
        }
        Err(TmuxError::failed("source-file", &output).into())
    }

    fn verify(tmux: &TmuxCommand, saved_line: &str) -> Result<(), BindingError> {
        let expected = KeyBinding::find_return_key(saved_line);
        let current = Self::return_key(tmux)?;
        match (expected, current) {
            (Some(expected), Some(current)) if expected.same_fields_as(&current) => Ok(()),
            _ => Err(BindingError::Failed {
                subcommand: "list-keys".to_string(),
                detail: "the restored Ctrl-Q binding does not match the saved one".to_string(),
            }),
        }
    }

    fn unbind(tmux: &TmuxCommand) -> Result<(), BindingError> {
        let unbind = TmuxSequence::new()
            .command(["unbind-key", "-T", "root", "C-q"])
            .command(["display-message", "-p", Self::DONE]);
        Self::require_done("unbind-key", &tmux.change(&unbind)?)
    }

    fn repair(&self, record: &BindingRecord) -> RecordFate {
        let tmux = self.tmux.on_socket(record.server().socket_path());
        let identity = match Self::server_identity(&tmux) {
            Ok(identity) => identity,
            Err(error) => return RecordFate::Keep(error.into()),
        };
        if identity.as_ref() != Some(record.server()) {
            return RecordFate::Discard;
        }
        match self.restore_on(&tmux, record) {
            Ok(()) => RecordFate::Discard,
            Err(error @ BindingError::NotRestored { .. }) => RecordFate::DiscardAfter(error),
            Err(error) => RecordFate::Keep(error),
        }
    }

    fn settle(&self, records: Vec<BindingRecord>) -> Result<(), BindingError> {
        let mut kept = Vec::new();
        let mut failure = None;
        for record in &records {
            match self.repair(record) {
                RecordFate::Discard => {}
                RecordFate::DiscardAfter(error) => failure = Some(error),
                RecordFate::Keep(error) => {
                    log::warn!(
                        "kept the saved Ctrl-Q binding {:?} for a later retry: {error}",
                        record.saved()
                    );
                    failure = Some(BindingError::RecordKept {
                        detail: error.to_string(),
                        saved_line: record.saved().unwrap_or_default().to_string(),
                    });
                    kept.push(record.clone());
                }
            }
        }
        if kept.len() != records.len() {
            self.records.write(kept)?;
        }
        failure.map_or(Ok(()), Err)
    }

    fn require_done(subcommand: &str, output: &TmuxOutput) -> Result<(), BindingError> {
        if output.has(Self::DONE) {
            return Ok(());
        }
        Err(TmuxError::failed(subcommand, output).into())
    }
}

impl ReturnBinding for TmuxReturnBinding {
    fn install(&self) -> Result<(), BindingError> {
        let return_branch = self.return_branch()?;
        let Some(server) = Self::server_identity(&self.tmux)? else {
            return Err(BindingError::Unavailable {
                reason: "no tmux server is running".to_string(),
            });
        };
        let current = Self::return_key(&self.tmux)?;
        let desired = KeyBinding::return_command(&self.tmux_prefix, &return_branch);
        if current
            .as_ref()
            .is_some_and(|binding| binding.command() == desired)
        {
            return Ok(());
        }
        let mut records = self.records.read().unwrap_or_default();
        if !records.iter().any(|record| record.server() == &server) {
            let saved = current
                .filter(|binding| !binding.is_ringleaders())
                .map(|binding| binding.line().to_string());
            records.push(BindingRecord::new(server, saved));
            self.records.write(records)?;
        }
        self.bind(&return_branch)
    }

    fn restore(&self) -> Result<(), BindingError> {
        self.settle(self.records.read().unwrap_or_default())
    }

    fn repair_stale(&self) -> Result<(), BindingError> {
        match self.records.read() {
            Ok(records) => self.settle(records),
            Err(problem) => {
                log::warn!(
                    "discarding {}, which is {problem}",
                    self.records.path().display()
                );
                self.records.write(Vec::new())
            }
        }
    }
}

impl From<TmuxError> for BindingError {
    fn from(error: TmuxError) -> Self {
        match error {
            TmuxError::Failed { subcommand, detail } => BindingError::Failed { subcommand, detail },
            TmuxError::TimedOut {
                subcommand,
                seconds,
            } => BindingError::TimedOut {
                subcommand,
                seconds,
            },
        }
    }
}

enum ReturnTarget {
    Detach,
    Pane(String),
    Unavailable,
}

enum RecordFate {
    Discard,
    DiscardAfter(BindingError),
    Keep(BindingError),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::test_support::TmuxTestServer;

    const USER_COMMAND: &str = "send-keys -X cancel";

    struct BindingFixture {
        server: TmuxTestServer,
        records: PathBuf,
    }

    impl BindingFixture {
        fn new() -> Self {
            let server = TmuxTestServer::with_session("holder");
            server.start_session("ringleader-a");
            let records = server.directory().join("bindings.json");
            Self { server, records }
        }

        fn outside(&self) -> TmuxReturnBinding {
            TmuxReturnBinding::new(self.server.command(), &self.records, false, "ringleader-")
        }

        fn inside(&self, pane: &str) -> TmuxReturnBinding {
            self.inside_with(Some(pane))
        }

        fn inside_with(&self, pane: Option<&str>) -> TmuxReturnBinding {
            let tmux_value = format!("{},0,0", self.server.socket().display());
            let mut extra = vec![("TMUX", tmux_value.as_str())];
            if let Some(pane) = pane {
                extra.push(("TMUX_PANE", pane));
            }
            let command = self
                .server
                .command_with(&self.server.environment_with(&extra));
            TmuxReturnBinding::new(command, &self.records, true, "ringleader-")
        }

        fn bind_user(&self) {
            self.server
                .text(&["bind-key", "-T", "root", "C-q", "send-keys", "-X", "cancel"]);
        }

        fn return_command(&self) -> Option<String> {
            let listing = self.server.text(&["list-keys"]);
            KeyBinding::find_return_key(&listing).map(|binding| binding.command().to_string())
        }

        fn tui_pane(&self) -> String {
            self.server.pane_of("=holder:")
        }
    }

    #[test]
    fn install_then_restore_brings_back_the_user_binding() {
        let fixture = BindingFixture::new();
        fixture.bind_user();
        let binding = fixture.outside();

        binding.install().unwrap();
        let installed = fixture.return_command().unwrap();
        binding.restore().unwrap();

        assert!(installed.contains("detach-client"));
        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
        assert!(!fixture.records.exists());
    }

    #[test]
    fn the_record_reaches_disk_with_the_saved_line() {
        let fixture = BindingFixture::new();
        fixture.bind_user();

        fixture.outside().install().unwrap();

        let record = fs::read_to_string(&fixture.records).unwrap();
        assert!(record.contains(USER_COMMAND));
    }

    #[test]
    fn two_installs_then_one_restore_bring_back_the_user_binding() {
        let fixture = BindingFixture::new();
        fixture.bind_user();
        let binding = fixture.outside();

        binding.install().unwrap();
        binding.install().unwrap();
        binding.restore().unwrap();

        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
    }

    #[test]
    fn a_longer_table_name_added_meanwhile_does_not_spoil_the_restore() {
        let fixture = BindingFixture::new();
        fixture.bind_user();
        let binding = fixture.outside();
        binding.install().unwrap();
        fixture.server.text(&[
            "bind-key",
            "-T",
            "a-much-longer-example-table-name",
            "M-F12",
            "send-keys",
            "wide",
        ]);

        binding.restore().unwrap();

        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
    }

    #[test]
    fn an_unbound_key_is_unbound_again_after_restore() {
        let fixture = BindingFixture::new();
        fixture.server.text(&["unbind-key", "-T", "root", "C-q"]);
        let binding = fixture.outside();

        binding.install().unwrap();
        binding.restore().unwrap();

        assert_eq!(fixture.return_command(), None);
    }

    #[test]
    fn a_root_table_holding_only_the_key_restores() {
        let fixture = BindingFixture::new();
        fixture.server.text(&["unbind-key", "-a", "-T", "root"]);
        fixture.bind_user();
        let binding = fixture.outside();

        binding.install().unwrap();
        binding.restore().unwrap();

        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
    }

    #[test]
    fn a_restore_after_a_server_restart_leaves_the_new_server_untouched() {
        let fixture = BindingFixture::new();
        fixture.bind_user();
        let binding = fixture.outside();
        binding.install().unwrap();
        fixture.server.text(&["kill-server"]);
        fixture.server.wait_until_gone();
        fixture.server.start_session("holder");
        fixture
            .server
            .text(&["bind-key", "-T", "root", "C-q", "send-keys", "new-server"]);

        binding.restore().unwrap();

        assert_eq!(fixture.return_command().unwrap(), "send-keys new-server");
        assert!(!fixture.records.exists());
    }

    #[test]
    fn repair_restores_the_binding_a_crashed_run_left() {
        let fixture = BindingFixture::new();
        fixture.bind_user();
        fixture.outside().install().unwrap();

        fixture.outside().repair_stale().unwrap();

        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
        assert!(!fixture.records.exists());
    }

    #[test]
    fn repair_keeps_the_record_when_tmux_cannot_answer() {
        let fixture = BindingFixture::new();
        let blocker = fixture.server.directory().join("regular-file");
        fs::write(&blocker, "not a directory").unwrap();
        let record = format!(
            "{{\"records\":[{{\"server\":{{\"socket_path\":\"{}\",\"pid\":\"1\"}},\"saved\":null}}]}}",
            blocker.join("socket").display()
        );
        fs::write(&fixture.records, &record).unwrap();

        let result = fixture.outside().repair_stale();

        assert!(matches!(result, Err(BindingError::RecordKept { .. })));
        assert_eq!(fs::read_to_string(&fixture.records).unwrap(), record);
    }

    #[test]
    fn an_empty_record_file_is_discarded() {
        let fixture = BindingFixture::new();
        fs::write(&fixture.records, "").unwrap();

        assert!(fixture.outside().repair_stale().is_ok());
        assert!(!fixture.records.exists());
    }

    #[test]
    fn a_record_for_another_server_process_is_discarded() {
        let fixture = BindingFixture::new();
        let record = format!(
            "{{\"records\":[{{\"server\":{{\"socket_path\":\"{}\",\"pid\":\"1\"}},\"saved\":null}}]}}",
            fixture.server.socket().display()
        );
        fs::write(&fixture.records, record).unwrap();

        fixture.outside().repair_stale().unwrap();

        assert!(!fixture.records.exists());
    }

    #[test]
    fn a_popup_without_a_pane_variable_installs_nothing() {
        let fixture = BindingFixture::new();
        fixture.bind_user();

        let result = fixture.inside_with(None).install();

        assert!(matches!(result, Err(BindingError::Unavailable { .. })));
        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
    }

    #[test]
    fn a_stale_pane_variable_aborts_the_install() {
        let fixture = BindingFixture::new();
        fixture.bind_user();

        let result = fixture.inside("%999").install();

        assert!(result.is_err());
        assert_eq!(fixture.return_command().unwrap(), USER_COMMAND);
    }

    #[test]
    fn outside_the_key_detaches_a_client_of_a_managed_session() {
        let fixture = BindingFixture::new();
        fixture.outside().install().unwrap();
        let mut client = fixture.server.control_client("=ringleader-a:");

        fixture.server.press(&client, "C-q");

        assert!(client.exits_with_notice_within(Duration::from_secs(2)));
    }

    #[test]
    fn inside_the_key_returns_a_client_to_the_tui_pane() {
        let fixture = BindingFixture::new();
        let tui_pane = fixture.tui_pane();
        fixture.inside(&tui_pane).install().unwrap();
        let client = fixture.server.control_client("=ringleader-a:");

        fixture.server.press(&client, "C-q");

        assert!(
            fixture
                .server
                .wait_for(|| fixture.server.client_pane(&client) == tui_pane)
        );
    }

    #[test]
    fn the_key_returns_after_switching_between_two_managed_sessions() {
        let fixture = BindingFixture::new();
        fixture.server.start_session("ringleader-b");
        let tui_pane = fixture.tui_pane();
        fixture.inside(&tui_pane).install().unwrap();
        let client = fixture.server.control_client("=ringleader-a:");
        let name = fixture.server.client_name(&client);
        fixture
            .server
            .text(&["switch-client", "-c", &name, "-t", "=ringleader-b:"]);

        fixture.server.press(&client, "C-q");

        assert!(
            fixture
                .server
                .wait_for(|| fixture.server.client_pane(&client) == tui_pane)
        );
    }

    #[test]
    fn the_key_still_reaches_an_unmanaged_session() {
        let fixture = BindingFixture::new();
        fixture.server.start_session("work");
        let work_pane = fixture.server.pane_of("=work:");
        fixture.inside(&fixture.tui_pane()).install().unwrap();
        let client = fixture.server.control_client("=work:");

        fixture.server.press(&client, "C-q");
        std::thread::sleep(Duration::from_millis(200));

        assert_eq!(fixture.server.client_pane(&client), work_pane);
    }

    #[test]
    fn the_key_returns_to_a_tui_pane_inside_a_managed_session() {
        let fixture = BindingFixture::new();
        let agent_pane = fixture.server.pane_of("=ringleader-a:");
        let tui_pane = fixture.server.text(&[
            "new-window",
            "-d",
            "-t",
            "=ringleader-a:",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "30",
        ]);
        fixture.inside(&tui_pane).install().unwrap();
        let client = fixture.server.control_client(&agent_pane);

        fixture.server.press(&client, "C-q");

        assert!(
            fixture
                .server
                .wait_for(|| fixture.server.client_pane(&client) == tui_pane)
        );
    }
}
