use std::time::Instant;

use super::home_view::LayoutTier;
use super::preview_panel::PreviewCapture;
use crate::application::SessionService;
use crate::domain::{SessionState, SessionSummary};

pub struct PreviewFeed {
    capture: Option<PreviewCapture>,
    tier: LayoutTier,
    next_refresh: Instant,
}

impl PreviewFeed {
    const STOPPED: &'static str = "the tmux session is stopped";

    pub fn new(now: Instant) -> Self {
        Self {
            capture: None,
            tier: LayoutTier::TooSmall,
            next_refresh: now,
        }
    }

    pub fn capture(&self) -> Option<&PreviewCapture> {
        self.capture.as_ref()
    }

    pub fn resize(&mut self, tier: LayoutTier) {
        self.tier = tier;
    }

    pub fn refresh_if_due(
        &mut self,
        now: Instant,
        service: &SessionService,
        row: Option<&SessionSummary>,
    ) {
        if now < self.next_refresh {
            return;
        }
        self.next_refresh = now + service.config().intervals().preview_refresh();
        self.refresh(service, row);
    }

    pub fn refresh(&mut self, service: &SessionService, row: Option<&SessionSummary>) {
        self.capture = row
            .filter(|_| self.tier.shows_preview())
            .map(|row| Self::captured(service, row));
    }

    fn captured(service: &SessionService, row: &SessionSummary) -> PreviewCapture {
        if row.state() == SessionState::Stopped {
            return PreviewCapture::Unavailable(Self::STOPPED.to_string());
        }
        match service.preview(row.tmux_name()) {
            Ok(text) => PreviewCapture::Text(text),
            Err(error) => PreviewCapture::Unavailable(error.to_string()),
        }
    }
}
