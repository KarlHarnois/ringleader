use std::time::Instant;

use super::diff_queue::DiffQueue;
use super::home_view::LayoutTier;
use super::preview_feed::PreviewFeed;
use super::preview_panel::PreviewPanel;
use super::session_list::SessionList;
use super::session_table::SessionTable;
use super::status_bar::StatusNote;
use super::theme::Theme;
use crate::application::{ServiceError, SessionService};

pub struct HomeState {
    sessions: SessionList,
    preview: PreviewFeed,
    diffs: DiffQueue,
    table_rows: usize,
    next_observe: Instant,
}

impl HomeState {
    pub fn new(now: Instant) -> Self {
        Self {
            sessions: SessionList::default(),
            preview: PreviewFeed::new(now),
            diffs: DiffQueue::default(),
            table_rows: 0,
            next_observe: now,
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

    pub fn note(&self, tmux_prefix: &str) -> Option<String> {
        let row = self.sessions.selected()?;
        StatusNote::new(row, tmux_prefix).text()
    }

    pub fn resize(&mut self, width: u16, height: u16, service: &SessionService) {
        let tier = LayoutTier::for_size(width, height);
        self.table_rows = tier.table_rows(height);
        self.preview.resize(tier);
        self.preview.refresh(service, self.sessions.selected());
    }

    pub fn refresh_diffs(&mut self) {
        self.diffs.refresh_soon();
    }

    pub fn tick(&mut self, now: Instant, service: &mut SessionService) -> Result<(), ServiceError> {
        let mut observed = Ok(());
        if now >= self.next_observe {
            self.next_observe = now + service.config().intervals().state_poll();
            observed = self.observe(service);
        }
        self.preview
            .refresh_if_due(now, service, self.sessions.selected());
        let visible = self.sessions.visible_managed(self.table_rows);
        self.diffs.tick(now, &visible, service);
        self.sessions.apply_diffs(self.diffs.diffs());
        observed
    }

    pub fn observe(&mut self, service: &mut SessionService) -> Result<(), ServiceError> {
        let persist = service.read_only_reason().is_none();
        let rows = service.observe(persist)?;
        self.change_selection(service, |sessions| sessions.show(rows));
        self.diffs.keep_only(&self.sessions.names());
        self.sessions.apply_diffs(self.diffs.diffs());
        Ok(())
    }

    pub fn select(&mut self, name: &str, service: &SessionService) {
        self.change_selection(service, |sessions| sessions.select(name));
    }

    pub fn move_selection(&mut self, offset: isize, service: &SessionService) {
        self.change_selection(service, |sessions| sessions.move_selection(offset));
    }

    fn change_selection(
        &mut self,
        service: &SessionService,
        change: impl FnOnce(&mut SessionList),
    ) {
        let before = self.sessions.selected_name().map(str::to_string);
        change(&mut self.sessions);
        if self.sessions.selected_name() != before.as_deref() {
            self.preview.refresh(service, self.sessions.selected());
        }
    }
}
