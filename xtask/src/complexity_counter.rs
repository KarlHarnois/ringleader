use syn::visit::{self, Visit};
use syn::{BinOp, Block, ExprBinary, ExprForLoop, ExprIf, ExprLoop, ExprMatch, ExprTry, ExprWhile};

#[derive(Default)]
pub struct ComplexityCounter {
    decisions: usize,
}

impl ComplexityCounter {
    const SINGLE_PATH: usize = 1;

    pub fn of(body: &Block) -> usize {
        let mut counter = Self::default();
        counter.visit_block(body);
        Self::SINGLE_PATH + counter.decisions
    }

    fn is_short_circuit(operator: &BinOp) -> bool {
        matches!(operator, BinOp::And(_) | BinOp::Or(_))
    }
}

impl<'ast> Visit<'ast> for ComplexityCounter {
    fn visit_expr_if(&mut self, expression: &'ast ExprIf) {
        self.decisions += 1;
        visit::visit_expr_if(self, expression);
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.decisions += 1;
        visit::visit_expr_for_loop(self, expression);
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.decisions += 1;
        visit::visit_expr_while(self, expression);
    }

    fn visit_expr_loop(&mut self, expression: &'ast ExprLoop) {
        self.decisions += 1;
        visit::visit_expr_loop(self, expression);
    }

    fn visit_expr_match(&mut self, expression: &'ast ExprMatch) {
        self.decisions += expression.arms.len().saturating_sub(Self::SINGLE_PATH);
        visit::visit_expr_match(self, expression);
    }

    fn visit_expr_try(&mut self, expression: &'ast ExprTry) {
        self.decisions += 1;
        visit::visit_expr_try(self, expression);
    }

    fn visit_expr_binary(&mut self, expression: &'ast ExprBinary) {
        self.decisions += usize::from(Self::is_short_circuit(&expression.op));
        visit::visit_expr_binary(self, expression);
    }
}

#[cfg(test)]
mod tests {
    use super::ComplexityCounter;

    struct Body;

    impl Body {
        fn complexity(statements: &str) -> usize {
            let block = syn::parse_str(&format!("{{ {statements} }}")).expect("the body parses");
            ComplexityCounter::of(&block)
        }
    }

    #[test]
    fn straight_line_code_has_one_path() {
        assert_eq!(Body::complexity("let total = 1 + 2; total"), 1);
    }

    #[test]
    fn each_if_adds_a_path() {
        assert_eq!(Body::complexity("if a { 1 } else if b { 2 } else { 3 }"), 3);
        assert_eq!(Body::complexity("if let Some(x) = y { x } else { 0 }"), 2);
    }

    #[test]
    fn each_loop_adds_a_path() {
        assert_eq!(
            Body::complexity("for x in y {} while a {} while let Some(b) = c {} loop {}"),
            5
        );
    }

    #[test]
    fn each_match_arm_after_the_first_adds_a_path() {
        assert_eq!(Body::complexity("match x { 1 => a, 2 => b, _ => c }"), 3);
        assert_eq!(Body::complexity("match x { _ => a }"), 1);
    }

    #[test]
    fn each_question_mark_adds_a_path() {
        assert_eq!(Body::complexity("let a = b()?; c(a)?"), 3);
    }

    #[test]
    fn each_short_circuit_operator_adds_a_path() {
        assert_eq!(Body::complexity("a && b || c & d"), 3);
    }

    #[test]
    fn closures_count_toward_the_enclosing_body() {
        assert_eq!(
            Body::complexity("items.map(|item| if item { 1 } else { 2 })"),
            2
        );
    }

    #[test]
    fn let_else_and_macros_add_no_path() {
        assert_eq!(
            Body::complexity("let Some(x) = y else { return }; assert!(a && b)"),
            1
        );
    }
}
