use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCacheDir(PathBuf);

impl BuildCacheDir {
    pub fn parse(text: &str) -> Option<Self> {
        let path = Path::new(text);
        let stays_inside = path
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
        let names_a_directory = path.components().next().is_some();
        (stays_inside && names_a_directory).then(|| Self(path.to_path_buf()))
    }

    pub fn within(&self, worktree: &Path) -> PathBuf {
        worktree.join(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_directory_resolves_inside_the_worktree() {
        let target = BuildCacheDir::parse("target").unwrap();

        assert_eq!(
            target.within(Path::new("/worktrees/example-project/fix-auth")),
            PathBuf::from("/worktrees/example-project/fix-auth/target")
        );
    }

    #[test]
    fn a_nested_relative_directory_is_accepted() {
        assert!(BuildCacheDir::parse("web/node_modules").is_some());
    }

    #[test]
    fn an_absolute_directory_is_refused_because_it_would_land_outside_the_worktree() {
        assert_eq!(BuildCacheDir::parse("/tmp/target"), None);
    }

    #[test]
    fn a_directory_that_climbs_out_of_the_worktree_is_refused() {
        assert_eq!(BuildCacheDir::parse("../target"), None);
    }

    #[test]
    fn an_empty_directory_is_refused() {
        assert_eq!(BuildCacheDir::parse(""), None);
    }

    #[test]
    fn the_worktree_itself_is_refused() {
        assert_eq!(BuildCacheDir::parse("."), None);
    }
}
