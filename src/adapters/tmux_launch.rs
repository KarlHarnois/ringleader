use super::tmux_command::TmuxCommand;
use super::tmux_error::TmuxError;
use super::tmux_listing::TmuxListing;
use super::tmux_output::TmuxOutput;
use super::tmux_sequence::TmuxSequence;

pub struct TmuxLaunch<'a> {
    tmux: &'a TmuxCommand,
}

impl<'a> TmuxLaunch<'a> {
    pub const CREATED_LABEL: &'static str = "ringleader-created";
    pub const DONE_LABEL: &'static str = "ringleader-done";

    pub fn new(tmux: &'a TmuxCommand) -> Self {
        Self { tmux }
    }

    pub fn run(&self, sequence: &TmuxSequence, output: TmuxOutput) -> Result<Launched, TmuxError> {
        let subcommand = sequence.first_subcommand();
        let Some(created) = output.first(Self::CREATED_LABEL).and_then(Created::parse) else {
            return Err(TmuxError::failed(&subcommand, &output));
        };
        let Some(done) = output.first(Self::DONE_LABEL) else {
            self.kill(&created);
            return Err(TmuxError::failed(&subcommand, &output));
        };
        Self::warn_about_hooks(&subcommand, &output);
        Ok(Launched {
            done: done.to_string(),
            created,
        })
    }

    pub fn settle_agent_tag(
        &self,
        launched: &Launched,
        current_pane: &str,
    ) -> Result<(), TmuxError> {
        if current_pane == launched.created.pane {
            return Ok(());
        }
        let created_pane = launched.created.pane.as_str();
        let moving = TmuxSequence::new()
            .command([
                "set-option",
                "-p",
                "-u",
                "-t",
                current_pane,
                TmuxListing::AGENT_TAG,
            ])
            .command([
                "set-option",
                "-p",
                "-u",
                "-t",
                current_pane,
                "remain-on-exit",
            ])
            .command([
                "set-option",
                "-p",
                "-t",
                created_pane,
                TmuxListing::AGENT_TAG,
                "1",
            ])
            .command([
                "set-option",
                "-p",
                "-t",
                created_pane,
                "remain-on-exit",
                "on",
            ])
            .command([
                "display-message",
                "-p",
                "-t",
                created_pane,
                Self::DONE_LABEL,
            ]);
        let output = self.tmux.change(&moving);
        match output {
            Ok(output) if output.has(Self::DONE_LABEL) => {
                Self::warn_about_hooks("set-option", &output);
                Ok(())
            }
            Ok(output) => {
                self.kill(&launched.created);
                Err(TmuxError::failed("set-option", &output))
            }
            Err(error) => {
                self.kill(&launched.created);
                Err(error)
            }
        }
    }

    pub fn roll_back(&self, launched: &Launched) {
        self.kill(&launched.created);
    }

    fn kill(&self, created: &Created) {
        let command = if created.container.starts_with('$') {
            "kill-session"
        } else {
            "kill-window"
        };
        let kill = TmuxSequence::new().command([command, "-t", created.container.as_str()]);
        if let Err(error) = self.tmux.change(&kill) {
            log::warn!("could not roll back {}: {error}", created.container);
        }
    }

    fn warn_about_hooks(subcommand: &str, output: &TmuxOutput) {
        if output.succeeded() {
            return;
        }
        log::warn!(
            "tmux {subcommand} succeeded but exited nonzero, likely from a user hook: {} {}",
            output.unlabeled().join(" "),
            output.stderr()
        );
    }
}

pub struct Launched {
    created: Created,
    done: String,
}

impl Launched {
    pub fn container(&self) -> &str {
        &self.created.container
    }

    pub fn done_values(&self) -> Vec<&str> {
        self.done.splitn(2, ' ').collect()
    }
}

struct Created {
    container: String,
    pane: String,
}

impl Created {
    fn parse(values: &str) -> Option<Self> {
        let (container, pane) = values.split_once(' ')?;
        Some(Self {
            container: container.to_string(),
            pane: pane.to_string(),
        })
    }
}
