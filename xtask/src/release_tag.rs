use std::fmt;

use crate::version_bump::VersionBump;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseTag {
    major: u64,
    minor: u64,
    patch: u64,
}

impl ReleaseTag {
    const PREFIX: &'static str = "v";

    pub fn parse(tag: &str) -> Result<Self, String> {
        let invalid =
            || format!("Reading the release tag {tag:?} failed: expected v<major>.<minor>.<patch>");
        let version = tag.strip_prefix(Self::PREFIX).ok_or_else(invalid)?;
        let numbers: Vec<u64> = version
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|_| invalid())?;
        match numbers[..] {
            [major, minor, patch] => Ok(Self {
                major,
                minor,
                patch,
            }),
            _ => Err(invalid()),
        }
    }

    pub fn bumped(self, bump: VersionBump) -> Self {
        match bump {
            VersionBump::Major => Self {
                major: self.major + 1,
                minor: 0,
                patch: 0,
            },
            VersionBump::Minor => Self {
                minor: self.minor + 1,
                patch: 0,
                ..self
            },
            VersionBump::Patch => Self {
                patch: self.patch + 1,
                ..self
            },
        }
    }
}

impl fmt::Display for ReleaseTag {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}{}.{}.{}",
            Self::PREFIX,
            self.major,
            self.minor,
            self.patch
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseTag;
    use crate::version_bump::VersionBump;

    struct ReleaseFixture;

    impl ReleaseFixture {
        fn next(tag: &str, bump: VersionBump) -> String {
            ReleaseTag::parse(tag).unwrap().bumped(bump).to_string()
        }
    }

    #[test]
    fn a_patch_bump_raises_only_the_patch() {
        assert_eq!(ReleaseFixture::next("v0.1.4", VersionBump::Patch), "v0.1.5");
    }

    #[test]
    fn a_minor_bump_resets_the_patch() {
        assert_eq!(ReleaseFixture::next("v0.1.4", VersionBump::Minor), "v0.2.0");
    }

    #[test]
    fn a_major_bump_resets_the_minor_and_the_patch() {
        assert_eq!(ReleaseFixture::next("v0.2.3", VersionBump::Major), "v1.0.0");
    }

    #[test]
    fn a_tag_without_the_prefix_is_refused() {
        assert!(ReleaseTag::parse("0.1.0").is_err());
    }

    #[test]
    fn a_tag_without_three_numbers_is_refused() {
        assert!(ReleaseTag::parse("v0.1").is_err());
    }

    #[test]
    fn a_tag_with_a_suffix_is_refused() {
        assert!(ReleaseTag::parse("v0.1.0-rc1").is_err());
    }
}
