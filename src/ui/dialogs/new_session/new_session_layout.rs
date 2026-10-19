use ratatui::layout::{Constraint, Flex, Layout, Rect};

pub struct NewSessionLayout {
    area: Rect,
    error_rows: usize,
    fixed_upper_rows: usize,
    repo_floor: usize,
    status_rows: usize,
    wanted_prompt_rows: usize,
}

impl NewSessionLayout {
    const PROMPT_FLOOR: usize = 1;
    const BOX_FRAME_ROWS: usize = 2;
    const SPACER_ROWS: usize = 1;
    const ERROR_PLACEHOLDER_ROWS: usize = 1;
    const HINT_ROWS: usize = 1;
    const FEWEST_STATUS_ROWS: usize = 1;

    pub fn new(area: Rect, error_rows: usize) -> Self {
        Self {
            area,
            error_rows,
            fixed_upper_rows: 0,
            repo_floor: 0,
            status_rows: Self::FEWEST_STATUS_ROWS,
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

    pub fn with_status_rows(self, status_rows: usize) -> Self {
        Self {
            status_rows,
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
        self.repo_floor + self.rows_after_prompt() - self.placeholder_rows()
    }

    pub fn areas(&self, upper_rows: usize) -> [Rect; 5] {
        let heights = self.heights_that_fit(upper_rows).map(Self::length);
        let [upper, _, prompt_box, status, errors, _, hints] =
            Layout::vertical(heights).flex(Flex::Start).areas(self.area);
        [upper, prompt_box, status, errors, hints]
    }

    fn heights_that_fit(&self, upper_rows: usize) -> [usize; 7] {
        let box_rows = self.box_rows();
        let kept_rows = box_rows + self.status_rows + Self::HINT_ROWS;
        let room = self.height().saturating_sub(kept_rows);
        let error_rows = self.error_rows.min(room);
        let upper_rows = upper_rows.min(room.saturating_sub(error_rows));
        [
            upper_rows,
            self.spacer_rows(),
            box_rows,
            self.status_rows,
            error_rows,
            self.placeholder_rows(),
            Self::HINT_ROWS,
        ]
    }

    fn box_rows(&self) -> usize {
        self.prompt_rows() + Self::BOX_FRAME_ROWS
    }

    fn prompt_rows(&self) -> usize {
        self.wanted_prompt_rows
            .min(self.shared_rows())
            .max(Self::PROMPT_FLOOR)
    }

    fn shared_rows(&self) -> usize {
        self.spare_rows() - self.spacer_rows()
    }

    fn rows_after_prompt(&self) -> usize {
        self.shared_rows().saturating_sub(self.prompt_rows())
    }

    fn spacer_rows(&self) -> usize {
        let room = self.spare_rows().saturating_sub(Self::PROMPT_FLOOR);
        Self::SPACER_ROWS.min(room)
    }

    fn placeholder_rows(&self) -> usize {
        let wanted = Self::ERROR_PLACEHOLDER_ROWS.saturating_sub(self.error_rows);
        wanted.min(self.rows_after_prompt())
    }

    fn spare_rows(&self) -> usize {
        let settled = self.fixed_upper_rows
            + self.repo_floor
            + Self::BOX_FRAME_ROWS
            + self.status_rows
            + self.error_rows
            + Self::HINT_ROWS;
        self.height().saturating_sub(settled)
    }

    fn height(&self) -> usize {
        usize::from(self.area.height)
    }

    fn length(rows: usize) -> Constraint {
        Constraint::Length(u16::try_from(rows).unwrap_or(u16::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LayoutFixture {
        area_height: usize,
        error_rows: usize,
        status_rows: usize,
        wanted_prompt_rows: usize,
    }

    impl LayoutFixture {
        const WIDTH: u16 = 60;
        const FIXED_UPPER_ROWS: usize = 1;
        const REPO_FLOOR: usize = 1;
        const UPPER_ROWS: usize = Self::FIXED_UPPER_ROWS + Self::REPO_FLOOR;
        const BOX_FLOOR: usize = NewSessionLayout::PROMPT_FLOOR + NewSessionLayout::BOX_FRAME_ROWS;
        const FIELDS_AND_HINTS_HEIGHT: usize = Self::UPPER_ROWS
            + NewSessionLayout::BOX_FRAME_ROWS
            + NewSessionLayout::FEWEST_STATUS_ROWS
            + NewSessionLayout::HINT_ROWS;
        const ROOM_FOR_A_ONE_ROW_PROMPT: usize =
            Self::FIELDS_AND_HINTS_HEIGHT + NewSessionLayout::PROMPT_FLOOR;
        const TALL_HEIGHT: usize = 20;

        fn of_height(area_height: usize) -> Self {
            Self {
                area_height,
                error_rows: 0,
                status_rows: NewSessionLayout::FEWEST_STATUS_ROWS,
                wanted_prompt_rows: NewSessionLayout::PROMPT_FLOOR,
            }
        }

        fn tall() -> Self {
            Self::of_height(Self::TALL_HEIGHT)
        }

        fn with_error_rows(self, error_rows: usize) -> Self {
            Self { error_rows, ..self }
        }

        fn with_status_rows(self, status_rows: usize) -> Self {
            Self {
                status_rows,
                ..self
            }
        }

        fn wanting_prompt_rows(self, wanted_prompt_rows: usize) -> Self {
            Self {
                wanted_prompt_rows,
                ..self
            }
        }

        fn area(&self) -> Rect {
            let height = u16::try_from(self.area_height).unwrap();
            Rect::new(0, 0, Self::WIDTH, height)
        }

        fn layout(&self) -> NewSessionLayout {
            NewSessionLayout::new(self.area(), self.error_rows)
                .with_upper_rows(Self::FIXED_UPPER_ROWS, Self::REPO_FLOOR)
                .with_status_rows(self.status_rows)
                .with_prompt_rows(self.wanted_prompt_rows)
        }

        fn without_upper_rows(&self) -> NewSessionLayout {
            NewSessionLayout::new(self.area(), self.error_rows)
                .with_prompt_rows(self.wanted_prompt_rows)
        }

        fn areas(&self) -> [Rect; 5] {
            self.layout().areas(Self::UPPER_ROWS)
        }

        fn rows(area: Rect) -> usize {
            usize::from(area.height)
        }
    }

    #[test]
    fn the_box_grows_to_the_rows_the_prompt_wants_when_they_are_left() {
        let wanted = 4;

        let [_, prompt_box, _, _, _] = LayoutFixture::tall().wanting_prompt_rows(wanted).areas();

        assert_eq!(
            LayoutFixture::rows(prompt_box),
            wanted + NewSessionLayout::BOX_FRAME_ROWS
        );
    }

    #[test]
    fn a_layout_without_upper_rows_gives_the_prompt_every_spare_row() {
        let wanted = 5;
        let area_height = NewSessionLayout::BOX_FRAME_ROWS
            + NewSessionLayout::FEWEST_STATUS_ROWS
            + NewSessionLayout::HINT_ROWS
            + NewSessionLayout::SPACER_ROWS
            + wanted;
        let layout = LayoutFixture::of_height(area_height)
            .wanting_prompt_rows(wanted)
            .without_upper_rows();
        let no_upper_rows = 0;

        let [_, prompt_box, _, _, _] = layout.areas(no_upper_rows);

        assert_eq!(
            LayoutFixture::rows(prompt_box),
            wanted + NewSessionLayout::BOX_FRAME_ROWS
        );
        assert_eq!(layout.repo_room(), 0);
    }

    #[test]
    fn the_prompt_gets_only_the_rows_left_after_the_fields_the_error_and_the_hints() {
        let error_rows = 2;
        let rows_left = 3;
        let area_height = LayoutFixture::FIELDS_AND_HINTS_HEIGHT
            + error_rows
            + NewSessionLayout::SPACER_ROWS
            + rows_left;

        let [_, prompt_box, _, _, _] = LayoutFixture::of_height(area_height)
            .with_error_rows(error_rows)
            .wanting_prompt_rows(rows_left + 1)
            .areas();

        assert_eq!(
            LayoutFixture::rows(prompt_box),
            rows_left + NewSessionLayout::BOX_FRAME_ROWS
        );
    }

    #[test]
    fn the_prompt_keeps_one_row_when_nothing_is_left() {
        let [_, prompt_box, _, _, _] =
            LayoutFixture::of_height(LayoutFixture::FIELDS_AND_HINTS_HEIGHT)
                .wanting_prompt_rows(3)
                .areas();

        assert_eq!(LayoutFixture::rows(prompt_box), LayoutFixture::BOX_FLOOR);
    }

    #[test]
    fn the_repo_search_gets_its_floor_and_the_rows_the_prompt_leaves() {
        let error_rows = 1;
        let prompt_rows = 2;
        let rows_the_prompt_leaves = 4;
        let area_height = LayoutFixture::FIELDS_AND_HINTS_HEIGHT
            + error_rows
            + NewSessionLayout::SPACER_ROWS
            + prompt_rows
            + rows_the_prompt_leaves;

        let layout = LayoutFixture::of_height(area_height)
            .with_error_rows(error_rows)
            .wanting_prompt_rows(prompt_rows)
            .layout();

        assert_eq!(
            layout.repo_room(),
            LayoutFixture::REPO_FLOOR + rows_the_prompt_leaves
        );
    }

    #[test]
    fn a_tall_area_puts_a_blank_row_between_the_upper_rows_and_the_box() {
        let [upper, prompt_box, _, _, _] = LayoutFixture::tall().areas();

        assert_eq!(
            usize::from(prompt_box.y),
            usize::from(upper.bottom()) + NewSessionLayout::SPACER_ROWS
        );
    }

    #[test]
    fn a_short_area_keeps_the_hints_on_its_last_row() {
        let error_taller_than_the_area = LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT + 1;
        let fixture = LayoutFixture::of_height(LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT)
            .with_error_rows(error_taller_than_the_area)
            .wanting_prompt_rows(3);

        let [_, _, _, _, hints] = fixture.areas();

        assert_eq!(hints.bottom(), fixture.area().bottom());
        assert_eq!(LayoutFixture::rows(hints), NewSessionLayout::HINT_ROWS);
    }

    #[test]
    fn a_short_area_drops_the_spacer_and_the_empty_error_row_first() {
        let [upper, prompt_box, status, errors, hints] =
            LayoutFixture::of_height(LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT).areas();

        assert_eq!(LayoutFixture::rows(upper), LayoutFixture::UPPER_ROWS);
        assert_eq!(prompt_box.y, upper.bottom());
        assert_eq!(LayoutFixture::rows(errors), 0);
        assert_eq!(hints.y, status.bottom());
    }

    #[test]
    fn a_short_area_shrinks_the_upper_rows_before_the_error() {
        let error_rows = 1;

        let [upper, prompt_box, _, errors, _] =
            LayoutFixture::of_height(LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT)
                .with_error_rows(error_rows)
                .areas();

        assert_eq!(LayoutFixture::rows(errors), error_rows);
        assert_eq!(LayoutFixture::rows(prompt_box), LayoutFixture::BOX_FLOOR);
        assert_eq!(
            LayoutFixture::rows(upper),
            LayoutFixture::UPPER_ROWS - error_rows
        );
    }

    #[test]
    fn a_short_area_keeps_the_status_rows_under_the_box_while_the_upper_rows_shrink() {
        let error_rows = 1;

        let [upper, prompt_box, status, _, _] =
            LayoutFixture::of_height(LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT)
                .with_error_rows(error_rows)
                .areas();

        assert!(LayoutFixture::rows(upper) < LayoutFixture::UPPER_ROWS);
        assert_eq!(
            LayoutFixture::rows(status),
            NewSessionLayout::FEWEST_STATUS_ROWS
        );
        assert_eq!(status.y, prompt_box.bottom());
    }

    #[test]
    fn a_wrapped_status_line_keeps_both_rows_before_the_prompt_grows() {
        let status_rows = 2;

        let [_, prompt_box, status, _, _] =
            LayoutFixture::of_height(LayoutFixture::ROOM_FOR_A_ONE_ROW_PROMPT + 1)
                .with_status_rows(status_rows)
                .wanting_prompt_rows(3)
                .areas();

        assert_eq!(LayoutFixture::rows(status), status_rows);
        assert_eq!(LayoutFixture::rows(prompt_box), LayoutFixture::BOX_FLOOR);
    }

    #[test]
    fn the_status_rows_come_before_the_error_rows() {
        let status_rows = 2;

        let [_, _, status, errors, _] =
            LayoutFixture::of_height(LayoutFixture::FIELDS_AND_HINTS_HEIGHT)
                .with_status_rows(status_rows)
                .with_error_rows(1)
                .areas();

        assert_eq!(LayoutFixture::rows(status), status_rows);
        assert_eq!(LayoutFixture::rows(errors), 0);
    }

    #[test]
    fn without_an_error_the_row_kept_for_one_joins_the_gap_above_the_hints() {
        let [_, _, status, errors, hints] = LayoutFixture::tall().areas();
        let [_, _, errored_status, _, errored_hints] =
            LayoutFixture::tall().with_error_rows(1).areas();

        assert_eq!(LayoutFixture::rows(errors), 0);
        assert_eq!(
            hints.y - status.bottom(),
            errored_hints.y - errored_status.bottom()
        );
    }
}
