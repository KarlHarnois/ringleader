use std::cell::Cell;
use std::iter::once;
use std::ops::Range;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Style;
use ratatui::text::Line;

use super::dialog::{DialogOpening, DialogStep};
use crate::application::SessionService;
use crate::domain::{CpuPercent, MemorySize, ProcessLoad, SessionLoad, SessionSummary};
use crate::ui::background_jobs::resource_feed::{ResourceFeed, ResourceReading};
use crate::ui::table::viewport::Viewport;
use crate::ui::text_fit::TextFit;
use crate::ui::theme::Theme;

#[derive(Default)]
pub struct ResourceView {
    feed: ResourceFeed,
    selected: Option<String>,
    viewport: Cell<Viewport>,
}

impl ResourceView {
    pub const TITLE: &'static str = " Resources ";
    const HINTS: &'static str = "n diagnose · ↑ ↓ j k choose · U Esc q close";
    const MEASURING: &'static str = "Measuring CPU and memory…";
    const NO_RUNNING_SESSION: &'static str = "No session is running.";
    const UNREADABLE: &'static str = "could not read the processes";
    const PROCESS_INDENT: &'static str = "  ";
    const CPU_WIDTH: usize = 8;
    const MEMORY_WIDTH: usize = 10;
    const MINIMUM_GAP: usize = 2;

    pub fn poll(
        &mut self,
        now: Instant,
        service: &SessionService,
        rows_to_sample: Option<&[SessionSummary]>,
    ) {
        self.feed.poll(now, service, rows_to_sample);
        self.keep_a_listed_session_selected();
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> DialogStep {
        match key.code {
            KeyCode::Char('U' | 'q') | KeyCode::Esc => return DialogStep::Close,
            KeyCode::Char('n') => return self.diagnose_selected(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            _ => {}
        }
        DialogStep::Stay
    }

    pub fn lines(&self, width: usize, height: usize, theme: &Theme) -> Vec<Line<'static>> {
        let room = height.saturating_sub(1);
        let mut lines = self.body(width, room, theme);
        lines.push(self.footer(width, theme));
        lines
    }

    fn diagnose_selected(&self) -> DialogStep {
        match self.selected_load() {
            Some(load) => DialogStep::Open(DialogOpening::Diagnosis(Box::new(load.clone()))),
            None => DialogStep::Stay,
        }
    }

    fn keep_a_listed_session_selected(&mut self) {
        if self.selected_index().is_none() {
            self.selected = self.loads().first().map(|load| load.name().to_string());
        }
    }

    fn move_selection(&mut self, offset: isize) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let last = self.loads().len().saturating_sub(1);
        let moved = index.saturating_add_signed(offset).min(last);
        self.selected = Some(self.loads()[moved].name().to_string());
    }

    fn selected_index(&self) -> Option<usize> {
        self.loads().iter().position(|load| self.is_selected(load))
    }

    fn selected_load(&self) -> Option<&SessionLoad> {
        self.loads().iter().find(|load| self.is_selected(load))
    }

    fn loads(&self) -> &[SessionLoad] {
        match self.feed.reading() {
            ResourceReading::Loads(loads) => loads,
            ResourceReading::Measuring | ResourceReading::Unreadable => &[],
        }
    }

    fn body(&self, width: usize, room: usize, theme: &Theme) -> Vec<Line<'static>> {
        match self.feed.reading() {
            ResourceReading::Measuring => vec![Line::styled(Self::MEASURING, theme.dim())],
            ResourceReading::Unreadable => Vec::new(),
            ResourceReading::Loads(loads) if loads.is_empty() => {
                vec![Line::styled(Self::NO_RUNNING_SESSION, theme.dim())]
            }
            ResourceReading::Loads(loads) => {
                let lines = self.blocks(loads, width, theme);
                self.window(lines, &self.selected_block(loads), room)
            }
        }
    }

    fn blocks(&self, loads: &[SessionLoad], width: usize, theme: &Theme) -> Vec<Line<'static>> {
        loads
            .iter()
            .flat_map(|load| self.block(load, width, theme))
            .collect()
    }

    fn window(
        &self,
        lines: Vec<Line<'static>>,
        selected: &Range<usize>,
        room: usize,
    ) -> Vec<Line<'static>> {
        let line_count = lines.len();
        let viewport = self
            .viewport
            .get()
            .with_room(room)
            .scrolled_to(selected.end.saturating_sub(1), line_count)
            .scrolled_to(selected.start, line_count);
        self.viewport.set(viewport);
        let shown = viewport.lines(line_count);
        lines
            .into_iter()
            .skip(shown.start)
            .take(shown.len())
            .collect()
    }

    fn selected_block(&self, loads: &[SessionLoad]) -> Range<usize> {
        let mut start = 0;
        for load in loads {
            let end = start + Self::block_height(load);
            if self.is_selected(load) {
                return start..end;
            }
            start = end;
        }
        0..0
    }

    fn block_height(load: &SessionLoad) -> usize {
        1 + load.busiest().len()
    }

    fn block(&self, load: &SessionLoad, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let processes = load
            .busiest()
            .iter()
            .map(|process| Self::process_line(process, width, theme));
        once(self.heading_line(load, width, theme))
            .chain(processes)
            .collect()
    }

    fn heading_line(&self, load: &SessionLoad, width: usize, theme: &Theme) -> Line<'static> {
        let figures = Self::figures(load.cpu(), load.memory());
        let heading = Self::row(load.name(), &figures, width);
        Line::styled(heading, self.heading_style(load, theme))
    }

    fn heading_style(&self, load: &SessionLoad, theme: &Theme) -> Style {
        if self.is_selected(load) {
            theme.emphasis().patch(theme.selected())
        } else {
            theme.emphasis()
        }
    }

    fn is_selected(&self, load: &SessionLoad) -> bool {
        self.selected.as_deref() == Some(load.name())
    }

    fn process_line(process: &ProcessLoad, width: usize, theme: &Theme) -> Line<'static> {
        let label = format!("{}{}", Self::PROCESS_INDENT, process.program());
        let figures = Self::figures(process.cpu(), process.memory());
        Line::styled(Self::row(&label, &figures, width), theme.base())
    }

    fn row(label: &str, figures: &str, width: usize) -> String {
        let label_width = width.saturating_sub(figures.chars().count());
        format!("{}{figures}", TextFit::new(label).padded(label_width))
    }

    fn figures(cpu: CpuPercent, memory: MemorySize) -> String {
        format!(
            "{cpu:>cpu_width$}{memory:>memory_width$}",
            cpu_width = Self::CPU_WIDTH,
            memory_width = Self::MEMORY_WIDTH
        )
    }

    fn footer(&self, width: usize, theme: &Theme) -> Line<'static> {
        let status = self.status();
        let used = status.chars().count() + Self::HINTS.chars().count();
        let gap = width.saturating_sub(used).max(Self::MINIMUM_GAP);
        let footer = format!("{status}{}{}", " ".repeat(gap), Self::HINTS);
        Line::styled(footer, theme.hint())
    }

    fn status(&self) -> &'static str {
        match self.feed.reading() {
            ResourceReading::Unreadable => Self::UNREADABLE,
            ResourceReading::Measuring | ResourceReading::Loads(_) => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossterm::event::KeyModifiers;

    use super::*;
    use crate::domain::{LoadSample, ProcessFigures, ProcessSample, SessionState};
    use crate::test_support::{Deadline, FakeMultiplexer, FakeProcessMeter, ServiceFixture};

    struct ViewFixture {
        service: SessionService,
        rows: Vec<SessionSummary>,
    }

    impl ViewFixture {
        const WIDTH: usize = 50;
        const HEIGHT: usize = 20;
        const MEBIBYTE: u64 = 1024 * 1024;
        const FIX_AUTH_PANE: u32 = 100;
        const DOCS_PANE: u32 = 200;

        fn sampling(samples: Vec<ProcessSample>) -> Self {
            Self::sampling_from(FakeProcessMeter::new().with_samples(samples))
        }

        fn sampling_from(meter: FakeProcessMeter) -> Self {
            let fix_auth = ServiceFixture::session("fix-auth");
            let docs = ServiceFixture::session("docs");
            let multiplexer = FakeMultiplexer::new()
                .with_pane_pids(fix_auth.tmux_name(), &[Self::FIX_AUTH_PANE])
                .with_pane_pids(docs.tmux_name(), &[Self::DOCS_PANE]);
            let services = ServiceFixture::new()
                .with_multiplexer(multiplexer)
                .with_process_meter(meter);
            Self {
                service: services.service(),
                rows: vec![
                    SessionSummary::managed(fix_auth, SessionState::Working, false),
                    SessionSummary::managed(docs, SessionState::Waiting, false),
                ],
            }
        }

        fn idle_then(later: Vec<ProcessFigures>) -> Self {
            let idle = later
                .iter()
                .map(|process| {
                    ProcessFigures::new(process.pid(), process.parent(), process.program())
                })
                .collect();
            Self::sampling(vec![
                ProcessSample::new(Duration::ZERO, idle),
                ProcessSample::new(Duration::from_secs(1), later),
            ])
        }

        fn pane_process(pid: u32, cpu_milliseconds: u64, mebibytes: u64) -> ProcessFigures {
            Self::process(pid, 1, cpu_milliseconds)
                .with_resident_memory(MemorySize::of_bytes(mebibytes * Self::MEBIBYTE))
        }

        fn process(pid: u32, parent: u32, cpu_milliseconds: u64) -> ProcessFigures {
            ProcessFigures::new(pid, parent, "example-program")
                .with_cpu_time(Duration::from_millis(cpu_milliseconds))
        }

        fn fix_auth_busier() -> Self {
            Self::idle_then(vec![
                Self::pane_process(Self::FIX_AUTH_PANE, 300, 2),
                Self::pane_process(Self::DOCS_PANE, 100, 1),
            ])
        }

        fn settled_after(&self, samples: u32) -> ResourceView {
            let mut view = ResourceView::default();
            let start = Instant::now();
            for sample in 0..samples {
                self.settle(&mut view, start + LoadSample::INTERVAL * sample);
            }
            view
        }

        fn settled(&self) -> ResourceView {
            self.settled_after(2)
        }

        fn settle(&self, view: &mut ResourceView, now: Instant) {
            view.poll(now, &self.service, Some(&self.rows));
            let settled = Deadline::for_background_work().wait_until(|| {
                view.poll(now, &self.service, Some(&self.rows));
                !view.feed.is_sampling()
            });
            assert!(settled, "the sample never finished");
        }

        fn rows(view: &ResourceView) -> Vec<String> {
            Self::rows_within(view, Self::HEIGHT)
        }

        fn rows_within(view: &ResourceView, height: usize) -> Vec<String> {
            view.lines(Self::WIDTH, height, &Theme::default())
                .iter()
                .map(ToString::to_string)
                .collect()
        }

        fn session_rows(view: &ResourceView) -> Vec<String> {
            Self::rows(view)
                .into_iter()
                .filter(|row| !row.starts_with(' '))
                .collect()
        }

        fn selected_rows(view: &ResourceView) -> Vec<String> {
            let theme = Theme::default();
            let selected = theme.emphasis().patch(theme.selected());
            view.lines(Self::WIDTH, Self::HEIGHT, &theme)
                .iter()
                .filter(|line| line.style == selected)
                .map(ToString::to_string)
                .collect()
        }

        fn press(view: &mut ResourceView, code: KeyCode) -> DialogStep {
            view.handle_key(&KeyEvent::new(code, KeyModifiers::NONE))
        }
    }

    #[test]
    fn each_running_session_gets_a_row_with_its_total_cpu_and_memory() {
        let rows = ViewFixture::session_rows(&ViewFixture::fix_auth_busier().settled());

        assert_eq!(
            rows[0],
            "fix-auth                           30.0%   2.0 MiB"
        );
        assert_eq!(
            rows[1],
            "docs                               10.0%   1.0 MiB"
        );
    }

    #[test]
    fn sessions_are_listed_busiest_first() {
        let fixture = ViewFixture::idle_then(vec![
            ViewFixture::pane_process(ViewFixture::FIX_AUTH_PANE, 100, 2),
            ViewFixture::pane_process(ViewFixture::DOCS_PANE, 600, 1),
        ]);

        let rows = ViewFixture::rows(&fixture.settled());

        assert_eq!(
            rows[0],
            "docs                               60.0%   1.0 MiB"
        );
    }

    #[test]
    fn the_three_busiest_processes_of_a_session_sit_under_it_busiest_first() {
        let pane = ViewFixture::FIX_AUTH_PANE;
        let fixture = ViewFixture::idle_then(vec![
            ProcessFigures::new(pane, 1, "zsh").with_cpu_time(Duration::from_millis(10)),
            ProcessFigures::new(101, pane, "node").with_cpu_time(Duration::from_millis(900)),
            ProcessFigures::new(102, pane, "cat").with_cpu_time(Duration::from_millis(1)),
            ProcessFigures::new(103, 101, "cargo").with_cpu_time(Duration::from_millis(400)),
        ]);

        let rows = ViewFixture::rows(&fixture.settled());

        let programs: Vec<&str> = rows[1..4]
            .iter()
            .map(|row| row.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(programs, ["node", "cargo", "zsh"]);
    }

    #[test]
    fn a_process_row_shows_its_program_cpu_and_memory() {
        let pane = ViewFixture::FIX_AUTH_PANE;
        let node = ProcessFigures::new(101, pane, "node")
            .with_cpu_time(Duration::from_millis(900))
            .with_resident_memory(MemorySize::of_bytes(512 * ViewFixture::MEBIBYTE));
        let fixture = ViewFixture::idle_then(vec![ViewFixture::process(pane, 1, 0), node]);

        let rows = ViewFixture::rows(&fixture.settled());

        assert_eq!(
            rows[1],
            "  node                             90.0%   512 MiB"
        );
    }

    #[test]
    fn before_the_second_sample_the_view_says_it_is_measuring() {
        let view = ViewFixture::fix_auth_busier().settled_after(1);

        assert_eq!(ViewFixture::rows(&view)[0], "Measuring CPU and memory…");
    }

    #[test]
    fn with_no_running_session_the_view_says_so() {
        let mut fixture = ViewFixture::fix_auth_busier();
        fixture.rows.clear();

        let rows = ViewFixture::rows(&fixture.settled());

        assert_eq!(rows[0], "No session is running.");
    }

    #[test]
    fn an_unreadable_process_table_leaves_the_view_without_rows() {
        let meter = FakeProcessMeter::new().unreadable("example denial of the view");
        let view = ViewFixture::sampling_from(meter).settled_after(1);

        assert_eq!(
            ViewFixture::rows(&view),
            ["could not read the processes  n diagnose · ↑ ↓ j k choose · U Esc q close"]
        );
    }

    #[test]
    fn the_busiest_session_starts_selected() {
        let view = ViewFixture::fix_auth_busier().settled();

        assert_eq!(
            ViewFixture::selected_rows(&view),
            ["fix-auth                           30.0%   2.0 MiB"]
        );
    }

    #[test]
    fn j_selects_the_next_session() {
        let mut view = ViewFixture::fix_auth_busier().settled();

        ViewFixture::press(&mut view, KeyCode::Char('j'));

        assert!(ViewFixture::selected_rows(&view)[0].starts_with("docs "));
    }

    #[test]
    fn k_on_the_first_session_keeps_it_selected() {
        let mut view = ViewFixture::fix_auth_busier().settled();

        ViewFixture::press(&mut view, KeyCode::Char('k'));

        assert!(ViewFixture::selected_rows(&view)[0].starts_with("fix-auth "));
    }

    #[test]
    fn the_selection_follows_its_session_when_the_order_changes() {
        let at = |seconds, fix_auth_milliseconds, docs_milliseconds| {
            ProcessSample::new(
                Duration::from_secs(seconds),
                vec![
                    ViewFixture::process(ViewFixture::FIX_AUTH_PANE, 1, fix_auth_milliseconds),
                    ViewFixture::process(ViewFixture::DOCS_PANE, 1, docs_milliseconds),
                ],
            )
        };
        let fixture = ViewFixture::sampling(vec![at(0, 0, 0), at(1, 500, 100), at(2, 600, 900)]);
        let mut view = fixture.settled();
        ViewFixture::press(&mut view, KeyCode::Char('j'));

        fixture.settle(&mut view, Instant::now() + LoadSample::INTERVAL * 2);

        let rows = ViewFixture::rows(&view);
        assert!(rows[0].starts_with("docs "));
        assert!(ViewFixture::selected_rows(&view)[0].starts_with("docs "));
    }

    #[test]
    fn a_selection_below_the_room_scrolls_into_view() {
        let mut view = ViewFixture::fix_auth_busier().settled();
        ViewFixture::rows_within(&view, 2);

        ViewFixture::press(&mut view, KeyCode::Char('j'));

        assert!(ViewFixture::rows_within(&view, 2)[0].starts_with("docs "));
    }

    #[test]
    fn n_asks_for_a_session_that_diagnoses_the_selected_session() {
        let mut view = ViewFixture::fix_auth_busier().settled();

        let step = ViewFixture::press(&mut view, KeyCode::Char('n'));

        let DialogStep::Open(DialogOpening::Diagnosis(load)) = step else {
            panic!("expected a diagnosis, got {step:?}");
        };
        assert_eq!(load.name(), "fix-auth");
    }

    #[test]
    fn n_diagnoses_the_session_the_selection_moved_to() {
        let mut view = ViewFixture::fix_auth_busier().settled();
        ViewFixture::press(&mut view, KeyCode::Char('j'));

        let step = ViewFixture::press(&mut view, KeyCode::Char('n'));

        let DialogStep::Open(DialogOpening::Diagnosis(load)) = step else {
            panic!("expected a diagnosis, got {step:?}");
        };
        assert_eq!(load.name(), "docs");
    }

    #[test]
    fn n_before_the_first_reading_asks_for_nothing() {
        let mut view = ViewFixture::fix_auth_busier().settled_after(1);

        assert_eq!(
            ViewFixture::press(&mut view, KeyCode::Char('n')),
            DialogStep::Stay
        );
    }

    #[test]
    fn escape_capital_u_and_q_close_the_view() {
        let mut view = ViewFixture::fix_auth_busier().settled();

        for code in [KeyCode::Esc, KeyCode::Char('U'), KeyCode::Char('q')] {
            assert_eq!(ViewFixture::press(&mut view, code), DialogStep::Close);
        }
    }

    #[test]
    fn another_key_keeps_the_view_open() {
        let mut view = ViewFixture::fix_auth_busier().settled();

        assert_eq!(
            ViewFixture::press(&mut view, KeyCode::Char('x')),
            DialogStep::Stay
        );
    }
}
