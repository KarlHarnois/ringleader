pub struct CompactDuration {
    seconds: u64,
}

impl CompactDuration {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * Self::MINUTE;
    const DAY: u64 = 24 * Self::HOUR;

    pub fn new(seconds: u64) -> Self {
        Self { seconds }
    }

    pub fn label(&self) -> String {
        let seconds = self.seconds;
        if seconds < Self::MINUTE {
            return "<1m".to_string();
        }
        if seconds < Self::HOUR {
            return format!("{}m", seconds / Self::MINUTE);
        }
        if seconds < Self::DAY {
            return format!(
                "{}h {}m",
                seconds / Self::HOUR,
                seconds % Self::HOUR / Self::MINUTE
            );
        }
        format!(
            "{}d {}h",
            seconds / Self::DAY,
            seconds % Self::DAY / Self::HOUR
        )
    }
}
