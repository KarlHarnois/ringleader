use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionName(Slug);

impl SessionName {
    pub fn new(input: &str) -> Result<Self, InvalidName> {
        Slug::from_text(input).map(Self)
    }

    pub fn from_slug(text: &str) -> Result<Self, InvalidName> {
        Slug::from_exact(text).map(Self)
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl Slug {
    pub const MAX_LENGTH: usize = 64;
    const SEPARATOR: char = '-';

    pub fn from_text(input: &str) -> Result<Self, InvalidName> {
        let slug = Self::slugify(input);
        if slug.is_empty() {
            return Err(InvalidName::Empty {
                input: input.to_string(),
            });
        }
        Ok(Self(slug))
    }

    pub fn from_exact(text: &str) -> Result<Self, InvalidName> {
        let slug = Self::from_text(text)?;
        if slug.as_str() != text {
            return Err(InvalidName::NotCanonical {
                input: text.to_string(),
                slug: slug.0,
            });
        }
        Ok(slug)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn slugify(input: &str) -> String {
        let mapped: String = input.to_lowercase().chars().map(Self::map_char).collect();
        let collapsed = Self::collapse_separators(&mapped);
        let truncated: String = Self::trim(&collapsed)
            .chars()
            .take(Self::MAX_LENGTH)
            .collect();
        Self::trim(&truncated).to_string()
    }

    fn map_char(character: char) -> char {
        if Self::is_allowed(character) {
            character
        } else {
            Self::SEPARATOR
        }
    }

    fn is_allowed(character: char) -> bool {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == Self::SEPARATOR
    }

    fn collapse_separators(text: &str) -> String {
        let mut collapsed = String::with_capacity(text.len());
        for character in text.chars() {
            let repeats_separator =
                character == Self::SEPARATOR && collapsed.ends_with(Self::SEPARATOR);
            if !repeats_separator {
                collapsed.push(character);
            }
        }
        collapsed
    }

    fn trim(text: &str) -> &str {
        text.trim_matches(Self::SEPARATOR)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InvalidName {
    #[error("name `{input}` has no letters or digits")]
    Empty { input: String },
    #[error("name `{input}` is not a valid slug, the slug would be `{slug}`")]
    NotCanonical { input: String, slug: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_the_input() {
        assert_eq!(SessionName::new("FixAuth").unwrap().as_str(), "fixauth");
    }

    #[test]
    fn maps_characters_outside_the_alphabet_to_separators() {
        assert_eq!(
            SessionName::new("fix_auth.v2").unwrap().as_str(),
            "fix-auth-v2"
        );
    }

    #[test]
    fn maps_colons_and_slashes_so_the_slug_is_a_legal_tmux_and_ref_name() {
        assert_eq!(SessionName::new("a:b/c").unwrap().as_str(), "a-b-c");
    }

    #[test]
    fn maps_non_ascii_letters_to_separators() {
        assert_eq!(
            SessionName::new("café au lait").unwrap().as_str(),
            "caf-au-lait"
        );
    }

    #[test]
    fn collapses_runs_of_separators() {
        assert_eq!(
            SessionName::new("fix   --  auth").unwrap().as_str(),
            "fix-auth"
        );
    }

    #[test]
    fn trims_separators_at_both_ends() {
        assert_eq!(
            SessionName::new("--fix auth!").unwrap().as_str(),
            "fix-auth"
        );
    }

    #[test]
    fn caps_the_slug_at_sixty_four_characters() {
        let long_input = "a".repeat(100);

        let name = SessionName::new(&long_input).unwrap();

        assert_eq!(name.as_str().len(), Slug::MAX_LENGTH);
    }

    #[test]
    fn trims_a_separator_left_at_the_cap() {
        let input = format!("{}-tail", "a".repeat(63));

        let name = SessionName::new(&input).unwrap();

        assert_eq!(name.as_str(), "a".repeat(63));
    }

    #[test]
    fn rejects_an_input_that_slugs_to_empty() {
        let error = SessionName::new("!!!").unwrap_err();

        assert_eq!(
            error,
            InvalidName::Empty {
                input: "!!!".to_string()
            }
        );
    }

    #[test]
    fn names_that_slug_to_the_same_string_collide() {
        let first = SessionName::new("Fix Auth").unwrap();
        let second = SessionName::new("fix_auth").unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn from_slug_accepts_a_canonical_slug() {
        assert_eq!(
            SessionName::from_slug("fix-auth").unwrap().as_str(),
            "fix-auth"
        );
    }

    #[test]
    fn from_slug_rejects_text_that_is_not_its_own_slug() {
        let error = SessionName::from_slug("Fix-Auth").unwrap_err();

        assert!(matches!(error, InvalidName::NotCanonical { .. }));
    }
}
