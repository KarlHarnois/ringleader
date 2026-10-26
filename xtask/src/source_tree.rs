use std::fs;
use std::path::{Path, PathBuf};

use syn::{Item, ItemMod};

use crate::function_collector::FunctionCollector;
use crate::gate_failure::GateFailure;
use crate::host_configuration::HostConfiguration;
use crate::source_function::SourceFunction;

pub struct SourceTree {
    root: PathBuf,
    host: HostConfiguration,
}

impl SourceTree {
    const CRATE_ROOT: &str = "src/main.rs";
    const DIRECTORY_OWNERS: [&str; 2] = ["main.rs", "mod.rs"];
    const MODULE_FILE: &str = "mod.rs";
    const EXTENSION: &str = "rs";

    pub fn new(root: &Path, host: HostConfiguration) -> Self {
        Self {
            root: root.to_path_buf(),
            host,
        }
    }

    pub fn functions(&self) -> Result<Vec<SourceFunction>, GateFailure> {
        self.functions_from(&self.root.join(Self::CRATE_ROOT))
    }

    fn functions_from(&self, path: &Path) -> Result<Vec<SourceFunction>, GateFailure> {
        let syntax = Self::parse(path)?;
        let mut functions = FunctionCollector::collect(self.relative(path), self.host, &syntax);
        for module in self.compiled_module_files(&syntax.items) {
            functions.extend(self.functions_from(&Self::module_path(path, &module)?)?);
        }
        Ok(functions)
    }

    fn parse(path: &Path) -> Result<syn::File, GateFailure> {
        let text =
            fs::read_to_string(path).map_err(|error| GateFailure::unreadable(path, error))?;
        syn::parse_file(&text).map_err(|error| GateFailure::unparsable(path, &error))
    }

    fn compiled_module_files(&self, items: &[Item]) -> Vec<PathBuf> {
        self.compiled_modules(items)
            .flat_map(|module| self.module_files_under(module))
            .collect()
    }

    fn compiled_modules<'a>(&self, items: &'a [Item]) -> impl Iterator<Item = &'a ItemMod> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Mod(module) => Some(module),
                _ => None,
            })
            .filter(|module| self.host.compiles(&module.attrs))
    }

    fn module_files_under(&self, module: &ItemMod) -> Vec<PathBuf> {
        let name = PathBuf::from(module.ident.to_string());
        match &module.content {
            None => vec![name],
            Some((_, inline_items)) => self
                .compiled_module_files(inline_items)
                .into_iter()
                .map(|nested| name.join(nested))
                .collect(),
        }
    }

    fn module_path(declaring_file: &Path, module: &Path) -> Result<PathBuf, GateFailure> {
        let directory = Self::child_directory(declaring_file);
        let candidates = [
            directory.join(module).with_extension(Self::EXTENSION),
            directory.join(module).join(Self::MODULE_FILE),
        ];
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                GateFailure::module_missing(declaring_file, &module.display().to_string())
            })
    }

    fn child_directory(declaring_file: &Path) -> PathBuf {
        let parent = declaring_file.parent().unwrap_or(Path::new(""));
        let owns_its_directory = declaring_file
            .file_name()
            .is_some_and(|name| Self::DIRECTORY_OWNERS.iter().any(|owner| name == *owner));
        match declaring_file.file_stem() {
            Some(stem) if !owns_its_directory => parent.join(stem),
            _ => parent.to_path_buf(),
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::SourceTree;
    use crate::host_configuration::HostConfiguration;

    struct SampleCrate {
        directory: TempDir,
    }

    impl SampleCrate {
        fn new() -> Self {
            Self {
                directory: TempDir::new().expect("a temporary directory"),
            }
        }

        fn with(self, path: &str, text: &str) -> Self {
            let file = self.directory.path().join(path);
            fs::create_dir_all(file.parent().unwrap_or(Path::new(""))).expect("the directory");
            fs::write(file, text).expect("the file");
            self
        }

        fn functions(&self) -> Vec<String> {
            self.scan().unwrap_or_else(|message| panic!("{message}"))
        }

        fn failure(&self) -> String {
            self.scan().err().unwrap_or_default()
        }

        fn scan(&self) -> Result<Vec<String>, String> {
            SourceTree::new(self.directory.path(), HostConfiguration::on("linux"))
                .functions()
                .map(|functions| functions.iter().map(ToString::to_string).collect())
                .map_err(|failure| failure.message().to_string())
        }
    }

    #[test]
    fn follows_module_files_and_module_directories() {
        let functions = SampleCrate::new()
            .with("src/main.rs", "mod shapes;\nmod adapters;\nfn main() {}\n")
            .with("src/shapes.rs", "mod circle;\nfn area() {}\n")
            .with("src/shapes/circle.rs", "fn radius() {}\n")
            .with("src/adapters/mod.rs", "fn adapt() {}\n")
            .functions();
        assert_eq!(
            functions,
            [
                "src/main.rs:3 main",
                "src/shapes.rs:2 area",
                "src/shapes/circle.rs:1 radius",
                "src/adapters/mod.rs:1 adapt",
            ]
        );
    }

    #[test]
    fn follows_module_files_declared_inside_inline_modules() {
        let functions = SampleCrate::new()
            .with(
                "src/main.rs",
                "mod platform {\n    mod linux;\n}\nfn main() {}\n",
            )
            .with("src/platform/linux.rs", "fn open() {}\n")
            .functions();
        assert_eq!(
            functions,
            ["src/main.rs:4 main", "src/platform/linux.rs:1 open"]
        );
    }

    #[test]
    fn skips_modules_the_host_does_not_compile() {
        let functions = SampleCrate::new()
            .with(
                "src/main.rs",
                "#[cfg(test)]\nmod test_support;\n#[cfg(target_os = \"macos\")]\nmod mac;\nfn main() {}\n",
            )
            .with("src/test_support.rs", "fn fake() {}\n")
            .with("src/mac.rs", "fn open() {}\n")
            .functions();
        assert_eq!(functions, ["src/main.rs:5 main"]);
    }

    #[test]
    fn a_declared_module_without_a_file_fails() {
        let failure = SampleCrate::new()
            .with("src/main.rs", "mod missing;\n")
            .failure();
        assert!(failure.contains("mod missing"));
    }
}
