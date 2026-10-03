use std::time::Instant;

use super::home_view::LayoutTier;
use super::preview_feed::PreviewFeed;
use super::preview_panel::PreviewCapture;
use super::session_list::SessionList;
use crate::application::{ServiceError, SessionService};

pub struct HomeState {
    sessions: SessionList,
    preview: PreviewFeed,
    next_observe: Instant,
}

impl HomeState {
    pub fn new(now: Instant) -> Self {
        Self {
            sessions: SessionList::default(),
            preview: PreviewFeed::new(now),
            next_observe: now,
        }
    }

    pub fn sessions(&self) -> &SessionList {
        &self.sessions
    }

    pub fn preview(&self) -> Option<&PreviewCapture> {
        self.preview.capture()
    }

    pub fn resize(&mut self, tier: LayoutTier, service: &SessionService) {
        self.preview.resize(tier);
        self.preview.refresh(service, self.sessions.selected());
    }

    pub fn tick(&mut self, now: Instant, service: &mut SessionService) -> Result<(), ServiceError> {
        let mut observed = Ok(());
        if now >= self.next_observe {
            self.next_observe = now + service.config().intervals().state_poll();
            observed = self.observe(service);
        }
        self.preview
            .refresh_if_due(now, service, self.sessions.selected());
        observed
    }

    pub fn observe(&mut self, service: &mut SessionService) -> Result<(), ServiceError> {
        let persist = service.read_only_reason().is_none();
        let rows = service.observe(persist)?;
        self.change_selection(service, |sessions| sessions.show(rows));
        Ok(())
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
