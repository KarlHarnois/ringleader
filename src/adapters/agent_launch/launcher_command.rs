use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use super::agent_handoff::AgentHandoff;
use super::handoff_contents::HandoffContents;
use crate::application::ports::AgentLaunch;

pub struct LauncherCommand {
    prefix: Vec<OsString>,
    handoff: AgentHandoff,
}

impl LauncherCommand {
    const HANDOFF_FLAG: &'static str = "--handoff";

    pub fn new(prefix: Vec<OsString>, handoff: AgentHandoff) -> Self {
        Self { prefix, handoff }
    }

    pub fn prepare(&self, launch: &AgentLaunch) -> io::Result<PreparedLaunch> {
        Ok(self.handing_off(self.handoff.write(launch)?))
    }

    pub fn prepare_relay(&self, contents: &HandoffContents) -> io::Result<PreparedLaunch> {
        Ok(self.handing_off(self.handoff.write_relay(contents)?))
    }

    fn handing_off(&self, handoff: PathBuf) -> PreparedLaunch {
        let mut words = self.prefix.clone();
        words.push(OsString::from(Self::HANDOFF_FLAG));
        words.push(handoff.clone().into_os_string());
        PreparedLaunch { words, handoff }
    }
}

pub struct PreparedLaunch {
    words: Vec<OsString>,
    handoff: PathBuf,
}

impl PreparedLaunch {
    pub fn words(&self) -> &[OsString] {
        &self.words
    }

    pub fn discard(&self) {
        AgentHandoff::delete(&self.handoff);
    }
}
