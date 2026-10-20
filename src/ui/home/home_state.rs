use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::bell::Bell;
use super::home_view::LayoutTier;
use super::key_hints::KeyHints;
use super::preview_panel::PreviewPanel;
use super::status_bar::{StatusBar, StatusNote};
use crate::application::ports::FoldStore;
use crate::application::{ServiceError, SessionService, Tool};
use crate::domain::{RepoGroup, SessionSummary};
use crate::ui::background_jobs::diff_queue::DiffQueue;
use crate::ui::background_jobs::observation_feed::ObservationFeed;
use crate::ui::background_jobs::preview_feed::PreviewFeed;
use crate::ui::background_jobs::pull_request_feed::PullRequestFeed;
use crate::ui::table::session_list::SessionList;
use crate::ui::table::session_table::SessionTable;
use crate::ui::theme::Theme;

pub struct HomeState {
    sessions: SessionList,
    folds: Box<dyn FoldStore>,
    bell: Bell,
    observations: ObservationFeed,
    preview: PreviewFeed,
    diffs: DiffQueue,
    pull_requests: PullRequestFeed,
    screen: (u16, u16),
    tmux_paused_until: Option<Instant>,
}

impl HomeState {
    pub const TMUX_PAUSE: Duration = Duration::from_secs(30);

    pub fn new(now: Instant, bell: Bell, folds: Box<dyn FoldStore>) -> Self {
        Self {
            sessions: SessionList::default(),
            folds,
            bell,
            observations: ObservationFeed::new(now),
            preview: PreviewFeed::new(now),
            diffs: DiffQueue::default(),
            pull_requests: PullRequestFeed::default(),
            screen: (0, 0),
            tmux_paused_until: None,
        }
    }

    pub fn restore_folds(&mut self) {
        match self.folds.load() {
            Ok(folded) => self.sessions.restore_folds(folded),
            Err(error) => log::warn!("could not restore the folded repos: {error}"),
        }
    }

    pub fn sessions(&self) -> &SessionList {
        &self.sessions
    }

    pub fn table(&self, tick: u64, theme: Theme) -> SessionTable<'_> {
        self.sessions.table(tick, theme)
    }

    pub fn preview_panel(&self, theme: Theme) -> PreviewPanel<'_> {
        let selected = self.sessions.selected();
        PreviewPanel::new(selected, self.preview.capture(selected), theme)
    }

    pub fn note(&self) -> Option<String> {
        let row = self.sessions.selected()?;
        StatusNote::new(row).text()
    }

    pub fn key_hints(&self) -> KeyHints {
        if self.sessions.heading_selected() {
            return KeyHints::on_heading();
        }
        match self.sessions.selected() {
            Some(_) => KeyHints::on_session(),
            None => KeyHints::without_selection(),
        }
    }

    pub fn resize(
        &mut self,
        width: u16,
        height: u16,
        status: &StatusBar,
        service: &SessionService,
    ) {
        self.screen = (width, height);
        self.fit_rows(status);
        self.preview.resize(LayoutTier::for_size(width, height));
        self.refresh_preview(service);
    }

    pub fn refresh_now(&mut self) {
        self.tmux_paused_until = None;
        self.refresh_diffs_soon();
        self.pull_requests.refresh_soon();
    }

    pub fn refresh_diffs_soon(&mut self) {
        self.diffs.refresh_soon();
    }

    pub fn pause_after(&mut self, error: &ServiceError) {
        if error.is_timeout_of(Tool::Tmux) {
            self.tmux_paused_until = Some(Instant::now() + Self::TMUX_PAUSE);
        }
    }

    pub fn awaits_results(&self) -> bool {
        self.observations.awaits_result()
            || self.preview.awaits_result()
            || self.diffs.awaits_results()
    }

    pub fn tick(
        &mut self,
        now: Instant,
        service: &mut SessionService,
        status: &StatusBar,
    ) -> Result<(), ServiceError> {
        self.fit_rows(status);
        let polled = self.poll(now, service);
        let visible = self.sessions.visible_managed();
        self.pull_requests.tick(now, &visible, service);
        self.sessions
            .apply_pull_requests(self.pull_requests.pull_requests());
        polled
    }

    pub fn poll(&mut self, now: Instant, service: &mut SessionService) -> Result<(), ServiceError> {
        let observed = self.poll_observations(now, service);
        self.poll_preview(now, service);
        let measured = self.poll_diffs(now, service);
        observed.and(measured)
    }

    pub fn observe(&mut self, service: &mut SessionService) -> Result<(), ServiceError> {
        self.observations.supersede(Instant::now(), service);
        if self.is_tmux_paused() {
            return Ok(());
        }
        let persist = Self::persists_observations(service);
        let rows = service
            .observe(persist)
            .inspect_err(|error| self.pause_after(error))?;
        self.show(rows, service);
        Ok(())
    }

    pub fn returned_from(&mut self, name: &str) {
        self.bell.forget(name);
    }

    pub fn select(&mut self, name: &str, service: &SessionService) {
        self.change_list(service, |sessions| sessions.select(name));
    }

    pub fn move_selection(&mut self, offset: isize, service: &SessionService) {
        self.change_list(service, |sessions| sessions.move_selection(offset));
    }

    pub fn select_first(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::select_first);
    }

    pub fn select_last(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::select_last);
    }

    pub fn select_high(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::select_high);
    }

    pub fn select_middle(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::select_middle);
    }

    pub fn select_low(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::select_low);
    }

    pub fn toggle_fold(&mut self, service: &SessionService) {
        self.change_list(service, SessionList::toggle_fold);
    }

    pub fn jump_to_repo(&mut self, number: usize, service: &SessionService) {
        self.change_list(service, |sessions| sessions.jump_to_repo(number));
    }

    fn change_list(&mut self, service: &SessionService, change: impl FnOnce(&mut SessionList)) {
        let selected_before = self.sessions.selected_name().map(str::to_string);
        let folded_before = self.sessions.folded().clone();
        change(&mut self.sessions);
        self.refresh_preview_if_moved_from(selected_before.as_deref(), service);
        self.save_folds_if_changed_from(&folded_before);
    }

    fn refresh_preview_if_moved_from(
        &mut self,
        selected_before: Option<&str>,
        service: &SessionService,
    ) {
        if self.sessions.selected_name() != selected_before {
            self.refresh_preview(service);
        }
    }

    fn save_folds_if_changed_from(&self, folded_before: &HashSet<RepoGroup>) {
        if self.sessions.folded() == folded_before {
            return;
        }
        if let Err(error) = self.folds.save(self.sessions.folded()) {
            log::warn!("could not save the folded repos: {error}");
        }
    }

    fn poll_observations(
        &mut self,
        now: Instant,
        service: &mut SessionService,
    ) -> Result<(), ServiceError> {
        let applied = self.apply_observations(service);
        if !self.is_tmux_paused() {
            self.observations.start_if_due(now, service);
        }
        applied
    }

    fn apply_observations(&mut self, service: &mut SessionService) -> Result<(), ServiceError> {
        let Some(finished) = self.observations.collect() else {
            return Ok(());
        };
        let persist = Self::persists_observations(service);
        let rows = finished
            .and_then(|observations| service.observe_with(observations, persist))
            .inspect_err(|error| self.pause_after(error))?;
        self.show(rows, service);
        Ok(())
    }

    fn show(&mut self, rows: Vec<SessionSummary>, service: &SessionService) {
        self.change_list(service, |sessions| sessions.show(rows));
        self.diffs.keep_only(&self.sessions.names());
        self.sessions.apply_diffs(self.diffs.diffs());
        self.pull_requests.keep_only(&self.sessions.names());
        self.sessions
            .apply_pull_requests(self.pull_requests.pull_requests());
        self.bell.watch(self.sessions.rows(), service);
    }

    fn poll_preview(&mut self, now: Instant, service: &SessionService) {
        if let Err(error) = self.preview.collect(self.sessions.selected()) {
            self.pause_after(&error);
        }
        if !self.is_tmux_paused() {
            self.preview
                .start_if_due(now, service, self.sessions.selected());
        }
    }

    fn poll_diffs(&mut self, now: Instant, service: &SessionService) -> Result<(), ServiceError> {
        let visible = self.sessions.visible_managed();
        let measured = self.diffs.poll(now, &visible, service);
        self.sessions.apply_diffs(self.diffs.diffs());
        measured
    }

    fn refresh_preview(&mut self, service: &SessionService) {
        if !self.is_tmux_paused() {
            self.preview.refresh(service, self.sessions.selected());
        }
    }

    fn persists_observations(service: &SessionService) -> bool {
        service.read_only_reason().is_none()
    }

    fn fit_rows(&mut self, status: &StatusBar) {
        self.sessions.resize(self.table_rows(status));
    }

    fn table_rows(&self, status: &StatusBar) -> usize {
        let (width, height) = self.screen;
        LayoutTier::for_size(width, height).table_rows(height, status.height(width, height))
    }

    fn is_tmux_paused(&self) -> bool {
        self.tmux_paused_until
            .is_some_and(|until| Instant::now() < until)
    }
}
