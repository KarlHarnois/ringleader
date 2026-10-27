use crate::domain::AgentPane;

pub struct PaneRow<'a> {
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

    pub fn parse(row: &'a str) -> Option<Self> {
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

    pub fn session(&self) -> &'a str {
        self.session
    }

    pub fn socket(&self) -> &'a str {
        self.socket
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    pub fn agent_tag(&self) -> &'a str {
        self.agent_tag
    }

    pub fn metadata(&self) -> &'a str {
        self.metadata
    }

    pub fn agent_pane(&self) -> AgentPane {
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
