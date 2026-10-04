use std::time::Instant;

use super::home_view::LayoutTier;
use super::preview_panel::PreviewCapture;
use crate::application::{ServiceError, SessionService};
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
    ) -> Result<(), ServiceError> {
        if now < self.next_refresh {
            return Ok(());
        }
        self.next_refresh = now + service.config().intervals().preview_refresh();
        self.refresh(service, row)
    }

    pub fn refresh(
        &mut self,
        service: &SessionService,
        row: Option<&SessionSummary>,
    ) -> Result<(), ServiceError> {
        let Some(row) = row.filter(|_| self.tier.shows_preview()) else {
            self.capture = None;
            return Ok(());
        };
        if row.state() == SessionState::Stopped {
            self.capture = Some(PreviewCapture::Unavailable(Self::STOPPED.to_string()));
            return Ok(());
        }
        let captured = service.preview(row.tmux_name());
        self.capture = Some(match &captured {
            Ok(text) => PreviewCapture::Text(text.clone()),
            Err(error) => PreviewCapture::Unavailable(error.to_string()),
        });
        captured.map(drop)
    }
}
