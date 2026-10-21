use unicode_width::UnicodeWidthChar;

pub struct AgentCursor {
    pane_dead: bool,
    in_mode: bool,
    shown: bool,
    column: usize,
    row: usize,
}

impl AgentCursor {
    const ON: &'static str = "1";

    pub fn parse(values: &str) -> Option<Self> {
        let [pane_dead, in_mode, shown, column, row] = values.split(' ').collect::<Vec<_>>()[..]
        else {
            return None;
        };
        Some(Self {
            pane_dead: pane_dead == Self::ON,
            in_mode: in_mode == Self::ON,
            shown: shown == Self::ON,
            column: column.parse().ok()?,
            row: row.parse().ok()?,
        })
    }

    pub fn is_dead(&self) -> bool {
        self.pane_dead
    }

    pub fn row(&self) -> usize {
        self.row
    }

    pub fn takes_keys(&self) -> bool {
        !self.pane_dead && !self.in_mode && self.shown
    }

    pub fn follows_a_bare_prompt_marker(&self, row: &str) -> bool {
        let before_cursor = self.cells_before_cursor(row);
        let marker = before_cursor.trim();
        !marker.is_empty()
            && !marker.chars().any(char::is_alphanumeric)
            && before_cursor.ends_with(char::is_whitespace)
    }

    fn cells_before_cursor(&self, row: &str) -> String {
        let mut before_cursor = String::new();
        let mut cells = 0;
        for character in row.chars() {
            cells += character.width().unwrap_or_default();
            if cells > self.column {
                return before_cursor;
            }
            before_cursor.push(character);
        }
        let trimmed_blanks = " ".repeat(self.column - cells);
        before_cursor + &trimmed_blanks
    }
}
