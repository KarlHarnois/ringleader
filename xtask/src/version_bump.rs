#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VersionBump {
    Patch,
    Minor,
    Major,
}

impl VersionBump {
    const MINOR_LABEL: &'static str = "release:minor";
    const MAJOR_LABEL: &'static str = "release:major";

    pub fn from_labels<'a>(labels: impl IntoIterator<Item = &'a str>) -> Self {
        labels
            .into_iter()
            .map(Self::from_label)
            .max()
            .unwrap_or(Self::Patch)
    }

    fn from_label(label: &str) -> Self {
        match label {
            Self::MAJOR_LABEL => Self::Major,
            Self::MINOR_LABEL => Self::Minor,
            _ => Self::Patch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VersionBump;

    #[test]
    fn no_label_bumps_the_patch() {
        assert_eq!(VersionBump::from_labels([]), VersionBump::Patch);
    }

    #[test]
    fn an_unrelated_label_bumps_the_patch() {
        assert_eq!(
            VersionBump::from_labels(["documentation"]),
            VersionBump::Patch
        );
    }

    #[test]
    fn the_minor_label_bumps_the_minor() {
        assert_eq!(
            VersionBump::from_labels(["release:minor"]),
            VersionBump::Minor
        );
    }

    #[test]
    fn the_major_label_bumps_the_major() {
        assert_eq!(
            VersionBump::from_labels(["release:major"]),
            VersionBump::Major
        );
    }

    #[test]
    fn the_largest_label_wins_when_several_pull_requests_ship_together() {
        let labels = ["release:minor", "release:major", "documentation"];

        assert_eq!(VersionBump::from_labels(labels), VersionBump::Major);
    }
}
