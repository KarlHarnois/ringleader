use ratatui::layout::{Constraint, Flex, Layout, Rect};

pub struct NewSessionLayout {
    area: Rect,
    error_rows: usize,
    fixed_upper_rows: usize,
    repo_floor: usize,
    wanted_prompt_rows: usize,
}

impl NewSessionLayout {
    const PROMPT_FLOOR: usize = 1;
    const SPACER_ROWS: usize = 1;
    const HINT_ROWS: usize = 1;

    pub fn new(area: Rect, error_rows: usize) -> Self {
        Self {
            area,
            error_rows,
            fixed_upper_rows: 0,
            repo_floor: 0,
            wanted_prompt_rows: Self::PROMPT_FLOOR,
        }
    }

    pub fn with_upper_rows(self, fixed_rows: usize, repo_floor: usize) -> Self {
        Self {
            fixed_upper_rows: fixed_rows,
            repo_floor,
            ..self
        }
    }

    pub fn with_prompt_rows(self, wanted: usize) -> Self {
        Self {
            wanted_prompt_rows: wanted,
            ..self
        }
    }

    pub fn repo_room(&self) -> usize {
        self.repo_floor + self.shared_rows().saturating_sub(self.prompt_rows())
    }

    pub fn areas(&self, upper_rows: usize) -> [Rect; 4] {
        let heights = self.heights_that_fit(upper_rows).map(Self::length);
        let [upper, prompt, _, errors, hints] =
            Layout::vertical(heights).flex(Flex::Start).areas(self.area);
        [upper, prompt, errors, hints]
    }

    fn heights_that_fit(&self, upper_rows: usize) -> [usize; 5] {
        let prompt_rows = self.prompt_rows();
        let room = self.height().saturating_sub(prompt_rows + Self::HINT_ROWS);
        let error_rows = self.error_rows.min(room);
        let spacer_rows = self.spacer_rows();
        let upper_rows = upper_rows.min(room.saturating_sub(error_rows + spacer_rows));
        [
            upper_rows,
            prompt_rows,
            spacer_rows,
            error_rows,
            Self::HINT_ROWS,
        ]
    }

    fn prompt_rows(&self) -> usize {
        self.wanted_prompt_rows
            .min(self.shared_rows())
            .max(Self::PROMPT_FLOOR)
    }

    fn shared_rows(&self) -> usize {
        self.spare_rows() - self.spacer_rows()
    }

    fn spacer_rows(&self) -> usize {
        let fits = self.spare_rows() >= Self::PROMPT_FLOOR + Self::SPACER_ROWS;
        if fits { Self::SPACER_ROWS } else { 0 }
    }

    fn spare_rows(&self) -> usize {
        let settled = self.fixed_upper_rows + self.repo_floor + self.error_rows + Self::HINT_ROWS;
        self.height().saturating_sub(settled)
    }

    fn height(&self) -> usize {
        usize::from(self.area.height)
    }

    fn length(rows: usize) -> Constraint {
        Constraint::Length(u16::try_from(rows).unwrap_or(u16::MAX))
    }
}
