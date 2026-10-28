pub struct BuildVersion {
    text: &'static str,
}

impl BuildVersion {
    pub const CURRENT: Self = Self::of_description(match option_env!("RINGLEADER_VERSION") {
        Some(description) => description,
        None => env!("CARGO_PKG_VERSION"),
    });
    const TAG_PREFIX: u8 = b'v';

    pub const fn of_description(description: &'static str) -> Self {
        let text = match description.as_bytes() {
            [Self::TAG_PREFIX, version @ ..] => match std::str::from_utf8(version) {
                Ok(version) => version,
                Err(_) => description,
            },
            _ => description,
        };
        Self { text }
    }

    pub const fn text(&self) -> &'static str {
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_description_drops_the_tag_prefix_to_match_homebrew_builds() {
        assert_eq!(
            BuildVersion::of_description("v0.2.3-4-gabc1234-dirty").text(),
            "0.2.3-4-gabc1234-dirty"
        );
    }

    #[test]
    fn a_crate_version_is_kept_whole() {
        assert_eq!(
            BuildVersion::of_description("0.1.0-gabc1234").text(),
            "0.1.0-gabc1234"
        );
    }
}
