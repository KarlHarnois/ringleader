use ratatui::text::{Line, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogLabels {
    abbreviated: bool,
}

impl DialogLabels {
    const FULL_WIDTH: usize = 10;
    const ABBREVIATED_WIDTH: usize = 2;

    pub fn full() -> Self {
        Self { abbreviated: false }
    }

    pub fn abbreviated() -> Self {
        Self { abbreviated: true }
    }

    pub fn width(self) -> usize {
        if self.abbreviated {
            Self::ABBREVIATED_WIDTH
        } else {
            Self::FULL_WIDTH
        }
    }

    pub fn labeled(self, label: &str, lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let shown: String = if self.abbreviated {
            label.chars().take(1).collect()
        } else {
            label.to_string()
        };
        let width = self.width();
        lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let prefix = if index == 0 { shown.as_str() } else { "" };
                let mut spans = vec![Span::raw(format!("{prefix:<width$}"))];
                spans.extend(
                    line.spans
                        .into_iter()
                        .map(|span| span.patch_style(line.style)),
                );
                Line::from(spans)
            })
            .collect()
    }
}
