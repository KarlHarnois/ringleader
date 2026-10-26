use syn::visit::{self, Visit};
use syn::{Block, Ident, ImplItemFn, ItemFn, ItemImpl, ItemMod, ItemTrait, Signature, TraitItemFn};

use crate::complexity_counter::ComplexityCounter;
use crate::host_configuration::HostConfiguration;
use crate::source_function::{LineSpan, SourceFunction};

pub struct FunctionCollector {
    file: String,
    host: HostConfiguration,
    owner: Option<String>,
    functions: Vec<SourceFunction>,
}

impl FunctionCollector {
    pub fn collect(
        file: String,
        host: HostConfiguration,
        syntax: &syn::File,
    ) -> Vec<SourceFunction> {
        let mut collector = Self {
            file,
            host,
            owner: None,
            functions: Vec::new(),
        };
        collector.visit_file(syntax);
        collector.functions
    }

    fn record(&mut self, signature: &Signature, body: &Block) {
        self.functions.push(SourceFunction::new(
            self.file.clone(),
            self.qualified(&signature.ident),
            Self::span(signature, body),
            ComplexityCounter::of(body),
        ));
    }

    fn span(signature: &Signature, body: &Block) -> LineSpan {
        LineSpan::new(
            signature.fn_token.span.start().line,
            body.brace_token.span.close().start().line,
        )
    }

    fn qualified(&self, name: &Ident) -> String {
        match &self.owner {
            Some(owner) => format!("{owner}::{name}"),
            None => name.to_string(),
        }
    }

    fn within_owner(&mut self, owner: Option<String>, visit: impl FnOnce(&mut Self)) {
        let outer = std::mem::replace(&mut self.owner, owner);
        visit(self);
        self.owner = outer;
    }

    fn type_name(implementation: &ItemImpl) -> Option<String> {
        match implementation.self_ty.as_ref() {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        }
    }
}

impl<'ast> Visit<'ast> for FunctionCollector {
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if self.host.compiles(&module.attrs) {
            visit::visit_item_mod(self, module);
        }
    }

    fn visit_item_fn(&mut self, function: &'ast ItemFn) {
        if self.host.compiles(&function.attrs) {
            self.record(&function.sig, &function.block);
        }
    }

    fn visit_item_impl(&mut self, implementation: &'ast ItemImpl) {
        if self.host.compiles(&implementation.attrs) {
            self.within_owner(Self::type_name(implementation), |collector| {
                visit::visit_item_impl(collector, implementation);
            });
        }
    }

    fn visit_item_trait(&mut self, definition: &'ast ItemTrait) {
        if self.host.compiles(&definition.attrs) {
            self.within_owner(Some(definition.ident.to_string()), |collector| {
                visit::visit_item_trait(collector, definition);
            });
        }
    }

    fn visit_impl_item_fn(&mut self, method: &'ast ImplItemFn) {
        if self.host.compiles(&method.attrs) {
            self.record(&method.sig, &method.block);
        }
    }

    fn visit_trait_item_fn(&mut self, method: &'ast TraitItemFn) {
        if let Some(body) = method
            .default
            .as_ref()
            .filter(|_| self.host.compiles(&method.attrs))
        {
            self.record(&method.sig, body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FunctionCollector;
    use crate::host_configuration::HostConfiguration;
    use crate::source_function::{LineSpan, SourceFunction};

    struct SampleFile;

    impl SampleFile {
        const TEXT: &str = "\
fn area(width: f64, height: f64) -> f64 {
    width * height
}

struct Square;

impl Square {
    fn side(&self) -> f64 {
        if true { 1.0 } else { 2.0 }
    }

    #[cfg(test)]
    fn fixture() -> Self {
        Square
    }
}

trait Shape {
    fn name(&self) -> String {
        String::new()
    }

    fn sides(&self) -> usize;
}

impl Shape for Square {
    fn sides(&self) -> usize {
        4
    }
}

#[cfg(target_os = \"macos\")]
impl Square {
    fn corner(&self) -> f64 {
        0.0
    }
}

#[cfg(test)]
mod tests {
    fn helper() {}
}
";

        fn names() -> Vec<String> {
            Self::collected()
                .iter()
                .map(|function| format!("{function} {}", function.complexity()))
                .collect()
        }

        fn collected() -> Vec<SourceFunction> {
            let syntax = syn::parse_file(Self::TEXT).expect("the sample parses");
            FunctionCollector::collect(
                "src/shapes.rs".to_string(),
                HostConfiguration::on("linux"),
                &syntax,
            )
        }
    }

    #[test]
    fn collects_compiled_functions_methods_and_trait_defaults_with_their_owner() {
        assert_eq!(
            SampleFile::names(),
            [
                "src/shapes.rs:1 area 1",
                "src/shapes.rs:8 Square::side 2",
                "src/shapes.rs:19 Shape::name 1",
                "src/shapes.rs:27 Square::sides 1",
            ]
        );
    }

    #[test]
    fn a_function_spans_its_signature_to_its_closing_brace() {
        assert_eq!(SampleFile::collected()[1].lines(), LineSpan::new(8, 10));
    }
}
