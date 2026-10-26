use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSpan {
    first: usize,
    last: usize,
}

impl LineSpan {
    pub fn new(first: usize, last: usize) -> Self {
        Self { first, last }
    }

    pub fn first(self) -> usize {
        self.first
    }

    pub fn last(self) -> usize {
        self.last
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SourceFunction {
    file: String,
    name: String,
    lines: LineSpan,
    complexity: usize,
}

impl SourceFunction {
    pub fn new(file: String, name: String, lines: LineSpan, complexity: usize) -> Self {
        Self {
            file,
            name,
            lines,
            complexity,
        }
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn lines(&self) -> LineSpan {
        self.lines
    }

    pub fn complexity(&self) -> usize {
        self.complexity
    }
}

impl fmt::Display for SourceFunction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{} {}",
            self.file, self.lines.first, self.name
        )
    }
}
