use std::time::Instant;

use super::home_view::LayoutTier;
use super::preview_panel::PreviewCapture;
use crate::application::{Pending, ServiceError, SessionService};
use crate::domain::{SessionState, SessionSummary};

pub struct PreviewFeed {
    shown: Option<SessionCapture>,
    capturing: Option<CaptureJob>,
    tier: LayoutTier,
    next_refresh: Instant,
}

impl PreviewFeed {
    const STOPPED: &'static str = "the tmux session is stopped";

    pub fn new(now: Instant) -> Self {
        Self {
            shown: None,
            capturing: None,
            tier: LayoutTier::TooSmall,
            next_refresh: now,
        }
    }

    pub fn capture(&self, row: Option<&SessionSummary>) -> Option<&PreviewCapture> {
        let shown = self.shown.as_ref()?;
        row.is_some_and(|row| shown.belongs_to(row))
            .then_some(&shown.capture)
    }

    pub fn awaits_result(&self) -> bool {
        self.capturing.is_some()
    }

    pub fn resize(&mut self, tier: LayoutTier) {
        self.tier = tier;
    }

    pub fn refresh(&mut self, service: &SessionService, row: Option<&SessionSummary>) {
        if self.capturing.is_none() {
            self.start(service, row);
        }
    }

    pub fn collect(&mut self, row: Option<&SessionSummary>) -> Result<(), ServiceError> {
        let Some((tmux_name, captured)) = self.take_finished() else {
            return Ok(());
        };
        if row.is_some_and(|row| row.tmux_name() == tmux_name) {
            self.shown = Some(SessionCapture::of(&tmux_name, &captured));
        }
        captured.map(drop)
    }

    pub fn start_if_due(
        &mut self,
        now: Instant,
        service: &SessionService,
        row: Option<&SessionSummary>,
    ) {
        if self.capturing.is_some() {
            return;
        }
        if now >= self.next_refresh {
            self.next_refresh = now + service.config().intervals().preview_refresh();
            self.start(service, row);
        } else if self.misses(row) {
            self.start(service, row);
        }
    }

    fn take_finished(&mut self) -> Option<(String, Result<String, ServiceError>)> {
        let captured = self.capturing.as_ref()?.pending.poll()?;
        let job = self.capturing.take()?;
        Some((job.tmux_name, captured))
    }

    fn misses(&self, row: Option<&SessionSummary>) -> bool {
        self.tier.shows_preview() && row.is_some() && self.capture(row).is_none()
    }

    fn start(&mut self, service: &SessionService, row: Option<&SessionSummary>) {
        let Some(row) = row.filter(|_| self.tier.shows_preview()) else {
            self.shown = None;
            return;
        };
        if row.state() == SessionState::Stopped {
            let stopped = PreviewCapture::Unavailable(Self::STOPPED.to_string());
            self.shown = Some(SessionCapture::new(row.tmux_name(), stopped));
            return;
        }
        self.capturing = Some(CaptureJob {
            tmux_name: row.tmux_name().to_string(),
            pending: service.start_preview(row.tmux_name()),
        });
    }
}

struct SessionCapture {
    tmux_name: String,
    capture: PreviewCapture,
}

impl SessionCapture {
    fn new(tmux_name: &str, capture: PreviewCapture) -> Self {
        Self {
            tmux_name: tmux_name.to_string(),
            capture,
        }
    }

    fn of(tmux_name: &str, captured: &Result<String, ServiceError>) -> Self {
        let capture = match captured {
            Ok(text) => PreviewCapture::Text(text.clone()),
            Err(error) => PreviewCapture::Unavailable(error.to_string()),
        };
        Self::new(tmux_name, capture)
    }

    fn belongs_to(&self, row: &SessionSummary) -> bool {
        self.tmux_name == row.tmux_name()
    }
}

struct CaptureJob {
    tmux_name: String,
    pending: Pending<String>,
}
