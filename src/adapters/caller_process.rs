use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerProcess {
    chain: Vec<ProcessStat>,
}

impl CallerProcess {
    pub fn read() -> Self {
        let mut chain = Vec::new();
        let mut pid = Some(std::process::id());
        while let Some(current) = pid.filter(|&pid| pid > 0) {
            let Some((stat, parent)) = ProcessStat::read(current) else {
                break;
            };
            chain.push(stat);
            pid = Some(parent);
        }
        Self { chain }
    }

    #[cfg(test)]
    pub fn from_chain(chain: Vec<ProcessStat>) -> Self {
        Self { chain }
    }

    pub fn descends_in_foreground(&self, pane_pid: u32, terminal: u64) -> bool {
        let Some(pane_index) = self
            .chain
            .iter()
            .position(|process| process.pid == pane_pid)
        else {
            return false;
        };
        let lineage = &self.chain[..=pane_index];
        lineage.iter().enumerate().any(|(index, process)| {
            let child_on_same_terminal = index > 0 && lineage[index - 1].terminal == terminal;
            process.terminal == terminal && process.in_foreground() && !child_on_same_terminal
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessStat {
    pid: u32,
    process_group: i64,
    terminal: u64,
    foreground_group: i64,
}

impl ProcessStat {
    #[cfg(test)]
    pub fn new(pid: u32, process_group: i64, terminal: u64, foreground_group: i64) -> Self {
        Self {
            pid,
            process_group,
            terminal,
            foreground_group,
        }
    }

    fn read(pid: u32) -> Option<(Self, u32)> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields: Vec<&str> = stat[stat.rfind(')')? + 2..].split(' ').collect();
        let parent = fields.get(1)?.parse().ok()?;
        let process = Self {
            pid,
            process_group: fields.get(2)?.parse().ok()?,
            terminal: fields.get(4)?.parse::<i64>().ok()? as u64,
            foreground_group: fields.get(5)?.parse().ok()?,
        };
        Some((process, parent))
    }

    fn in_foreground(&self) -> bool {
        self.process_group == self.foreground_group
    }
}
