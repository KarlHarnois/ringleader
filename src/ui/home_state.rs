use std::time::{Duration, Instant};

use super::bell::Bell;
use super::diff_queue::DiffQueue;
use super::home_view::LayoutTier;
use super::preview_feed::PreviewFeed;
use super::preview_panel::PreviewPanel;
use super::pull_request_feed::PullRequestFeed;
use super::session_list::SessionList;
use super::session_table::SessionTable;
use super::status_bar::StatusNote;
use super::theme::Theme;
use crate::application::{ServiceError, SessionService, Tool};

pub struct HomeState {
    sessions: SessionList,
    bell: Bell,
    preview: PreviewFeed,
    diffs: DiffQueue,
    pull_requests: PullRequestFeed,
    table_rows: usize,
    next_observe: Instant,
    tmux_paused_until: Option<Instant>,
}

impl HomeState {
    pub const TMUX_PAUSE: Duration = Duration::from_secs(30);

    pub fn new(now: Instant, bell: Bell) -> Self {
        Self {
            sessions: SessionList::default(),
            bell,
            preview: PreviewFeed::new(now),
            diffs: DiffQueue::default(),
            pull_requests: PullRequestFeed::default(),
            table_rows: 0,
            next_observe: now,
            tmux_paused_until: None,
        }
    }

    pub fn sessions(&self) -> &SessionList {
        &self.sessions
    }

    pub fn table(&self, tick: u64, theme: Theme) -> SessionTable<'_> {
        self.sessions.table(tick, theme)
    }

    pub fn preview_panel(&self, theme: Theme) -> PreviewPanel<'_> {
        PreviewPanel::new(self.sessions.selected(), self.preview.capture(), theme)
    }

    pub fn note(&self) -> Option<String> {
        let row = self.sessions.selected()?;
        StatusNote::new(row).text()
    }

    pub fn resize(&mut self, width: u16, height: u16, service: &SessionService) {
        let tier = LayoutTier::for_size(width, height);
        self.table_rows = tier.table_rows(height);
        self.preview.resize(tier);
        self.refresh_preview(service);
    }

    pub fn refresh_now(&mut self) {
        self.tmux_paused_until = None;
        self.diffs.refresh_soon();
        self.pull_requests.refresh_soon();
    }

    pub fn pause_after(&mut self, error: &ServiceError) {
        if error.is_timeout_of(Tool::Tmux) {
            self.tmux_paused_until = Some(Instant::now() + Self::TMUX_PAUSE);
        }
    }

    pub fn tick(&mut self, now: Instant, service: &mut SessionService) -> Result<(), ServiceError> {
        let mut observed = Ok(());
        if now >= self.next_observe && !self.is_tmux_paused() {
            self.next_observe = now + service.config().intervals().state_poll();
            observed = self.observe(service);
        }
        if !self.is_tmux_paused() {
            let previewed = self
                .preview
                .refresh_if_due(now, service, self.sessions.selected());
            self.pause_on_failure(previewed);
        }
        let visible = self.sessions.visible_managed(self.table_rows);
        let measured = self.diffs.tick(now, &visible, service);
        self.sessions.apply_diffs(self.diffs.diffs());
        self.pull_requests.tick(now, &visible, service);
        self.sessions
            .apply_pull_requests(self.pull_requests.pull_requests());
        observed.and(measured)
    }

    pub fn observe(&mut self, service: &mut SessionService) -> Result<(), ServiceError> {
        if self.is_tmux_paused() {
            return Ok(());
        }
        let persist = service.read_only_reason().is_none();
        let observed = service.observe(persist);
        if let Err(error) = &observed {
            self.pause_after(error);
        }
        let rows = observed?;
        self.change_selection(service, |sessions| sessions.show(rows));
        self.diffs.keep_only(&self.sessions.names());
        self.sessions.apply_diffs(self.diffs.diffs());
        self.pull_requests.keep_only(&self.sessions.names());
        self.sessions
            .apply_pull_requests(self.pull_requests.pull_requests());
        self.bell.watch(self.sessions.rows(), service);
        Ok(())
    }

    pub fn select(&mut self, name: &str, service: &SessionService) {
        self.change_selection(service, |sessions| sessions.select(name));
    }

    pub fn move_selection(&mut self, offset: isize, service: &SessionService) {
        self.change_selection(service, |sessions| sessions.move_selection(offset));
    }

    pub fn select_first(&mut self, service: &SessionService) {
        self.change_selection(service, SessionList::select_first);
    }

    pub fn select_last(&mut self, service: &SessionService) {
        self.change_selection(service, SessionList::select_last);
    }

    fn change_selection(
        &mut self,
        service: &SessionService,
        change: impl FnOnce(&mut SessionList),
    ) {
        let before = self.sessions.selected_name().map(str::to_string);
        change(&mut self.sessions);
        if self.sessions.selected_name() != before.as_deref() {
            self.refresh_preview(service);
        }
    }

    fn refresh_preview(&mut self, service: &SessionService) {
        if self.is_tmux_paused() {
            return;
        }
        let previewed = self.preview.refresh(service, self.sessions.selected());
        self.pause_on_failure(previewed);
    }

    fn pause_on_failure(&mut self, outcome: Result<(), ServiceError>) {
        if let Err(error) = outcome {
            self.pause_after(&error);
        }
    }

    fn is_tmux_paused(&self) -> bool {
        self.tmux_paused_until
            .is_some_and(|until| Instant::now() < until)
    }
}
