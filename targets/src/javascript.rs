//! JavaScript subset generation and reusable Tree-sitter validation.
//!
//! The grammar emits ordinary scripts with simple parameters, `let`/`const`,
//! objects, arrays, functions, and loops. Validation supplements Tree-sitter's
//! recovery parser with contextual checks for this subset. It is not a full
//! ECMAScript early-error implementation, and does not execute programs.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use anyhow::{Error, anyhow};
use core::convert::Infallible;
use fandango::Fandango;
use tree_sitter::{Node, Parser};

/// The source grammar; generated node types are exported alongside this marker.
#[derive(Fandango)]
#[fandango(grammar = "grammars/javascript.fan", parse = false)]
pub struct JavaScript(Infallible);

/// Separate validity penalties. Every field must be zero for acceptance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Validity {
    /// Tree-sitter error and missing nodes.
    pub syntax: usize,
    /// Conflicting lexical bindings in the same scope.
    pub bindings: usize,
    /// `const` declarations without an initializer (excluding for-in/of).
    pub initializers: usize,
    /// Invalid assignment or update targets.
    pub assignments: usize,
    /// Returns outside functions, including inside top-level blocks.
    pub returns: usize,
    /// Breaks or continues without a loop in the current function.
    pub loop_control: usize,
}

impl Validity {
    /// Whether all validity checks passed.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self == Self::default()
    }
}

/// Structural counts, measured on Tree-sitter's concrete syntax tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Function declarations.
    pub functions: usize,
    /// Statements, including declarations, excluding blocks and loop headers.
    pub statements: usize,
    /// Object literals.
    pub objects: usize,
    /// Array literals.
    pub arrays: usize,
    /// For, for-in/of, while, and do-while loops.
    pub loops: usize,
}

/// Soft size goals, independent of program acceptance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeGoals {
    /// Upper bound on function count.
    pub max_functions: usize,
    /// Lower bound on statement count.
    pub min_statements: usize,
    /// Lower bound on object literal count.
    pub min_objects: usize,
    /// Lower bound on array literal count.
    pub min_arrays: usize,
    /// Lower bound on loop count.
    pub min_loops: usize,
}

impl Default for SizeGoals {
    fn default() -> Self {
        Self {
            max_functions: 2,
            min_statements: 10,
            min_objects: 1,
            min_arrays: 1,
            min_loops: 1,
        }
    }
}

impl SizeGoals {
    /// Penalties in function, statement, object, array, loop order; lower is better.
    #[must_use]
    pub fn penalties(self, counts: Counts) -> [usize; 5] {
        [
            counts.functions.saturating_sub(self.max_functions),
            self.min_statements.saturating_sub(counts.statements),
            self.min_objects.saturating_sub(counts.objects),
            self.min_arrays.saturating_sub(counts.arrays),
            self.min_loops.saturating_sub(counts.loops),
        ]
    }
}

/// One parse's validity results and structural counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Analysis {
    /// Hard acceptance rules and individual validity objectives.
    pub validity: Validity,
    /// Counts used by the soft objectives.
    pub counts: Counts,
}

/// A parser reused across candidates; trees are released after each analysis.
pub struct Validator {
    parser: Parser,
}

impl Validator {
    /// Load the JavaScript grammar once.
    ///
    /// # Errors
    /// Returns an error if the Tree-sitter language ABI is incompatible.
    pub fn new() -> Result<Self, Error> {
        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_javascript::LANGUAGE.into())?;
        Ok(Self { parser })
    }

    /// Parse and check a source program without executing it.
    ///
    /// Scope/control-flow checks cover the generated subset, not arbitrary JSX,
    /// modules, classes, destructuring, labels, or strict-mode early errors.
    ///
    /// # Errors
    /// Returns an error if the configured parser fails to produce a tree.
    pub fn analyze(&mut self, source: &str) -> Result<Analysis, Error> {
        let tree = self
            .parser
            .parse(source, None)
            .ok_or_else(|| anyhow!("JavaScript parser did not produce a tree"))?;
        let mut checker = Checker {
            source,
            analysis: Analysis::default(),
            scopes: vec![BTreeMap::new()],
            enclosing_loops: Vec::new(),
            loops: 0,
        };
        // An iterative walk handles even deeply nested rejected source without
        // consuming the Rust stack. Include anonymous children: missing tokens
        // are not necessarily named nodes.
        let mut cursor = tree.walk();
        loop {
            checker.enter(cursor.node());
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                checker.leave(cursor.node());
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    if tree.root_node().has_error() && checker.analysis.validity.syntax == 0 {
                        checker.analysis.validity.syntax = 1;
                    }
                    return Ok(checker.analysis);
                }
            }
        }
    }
}

const LEXICAL: u8 = 1;
const FUNCTION: u8 = 2;
const PARAMETER: u8 = 4;

struct Checker<'a> {
    source: &'a str,
    analysis: Analysis,
    scopes: Vec<BTreeMap<String, u8>>,
    enclosing_loops: Vec<usize>,
    loops: usize,
}

fn is_loop(kind: &str) -> bool {
    matches!(
        kind,
        "for_statement" | "for_in_statement" | "while_statement" | "do_statement"
    )
}

fn is_function(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration" | "function_expression" | "arrow_function"
    )
}

fn is_function_body(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        is_function(parent.kind()) && parent.child_by_field_name("body") == Some(node)
    })
}

fn valid_assignment_target(mut node: Node<'_>) -> bool {
    while node.kind() == "parenthesized_expression" {
        if node.named_child_count() != 1 {
            return false;
        }
        node = node.named_child(0).expect("one child");
    }
    matches!(
        node.kind(),
        "identifier" | "member_expression" | "subscript_expression"
    ) && node.child_by_field_name("optional_chain").is_none()
}

impl Checker<'_> {
    fn bind(&mut self, name: Node<'_>, kind: u8) {
        let name = &self.source[name.byte_range()];
        let scope = self.scopes.last_mut().expect("script scope always exists");
        let previous = scope.entry(name.to_string()).or_default();
        // Simple duplicate parameters/functions are allowed in these non-strict
        // scripts. A lexical declaration conflicts with either, in either order.
        if *previous != 0 && (*previous & LEXICAL != 0 || kind == LEXICAL) {
            self.analysis.validity.bindings += 1;
        }
        *previous |= kind;
    }

    fn enter(&mut self, node: Node<'_>) {
        let kind = node.kind();
        if node.is_error() || node.is_missing() {
            self.analysis.validity.syntax += 1;
        }
        let parent_kind = node.parent().map_or("", |p| p.kind());
        if matches!(
            kind,
            "lexical_declaration"
                | "function_declaration"
                | "expression_statement"
                | "if_statement"
                | "return_statement"
                | "break_statement"
                | "continue_statement"
                | "empty_statement"
        ) || is_loop(kind)
        {
            // Initializers and omitted conditions in a for header are not
            // additional statements for the size objective.
            if parent_kind != "for_statement" {
                self.analysis.counts.statements += 1;
            }
        }
        if kind == "function_declaration"
            && let Some(name) = node.child_by_field_name("name")
        {
            self.bind(name, FUNCTION);
        }
        if is_function(kind) {
            self.analysis.counts.functions += 1;
            self.enclosing_loops.push(self.loops);
            self.loops = 0;
            self.scopes.push(BTreeMap::new());
            if let Some(params) = node.child_by_field_name("parameters") {
                for parameter in params.named_children(&mut params.walk()) {
                    if parameter.kind() == "identifier" {
                        self.bind(parameter, PARAMETER);
                    }
                }
            }
        }
        if (kind == "statement_block" && !is_function_body(node))
            || matches!(kind, "for_statement" | "for_in_statement")
        {
            self.scopes.push(BTreeMap::new());
        }
        if is_loop(kind) {
            self.analysis.counts.loops += 1;
            self.loops += 1;
            if kind == "for_in_statement"
                && node.child_by_field_name("kind").is_some()
                && let Some(left) = node.child_by_field_name("left")
            {
                self.bind(left, LEXICAL);
            }
        }
        match kind {
            "object" => self.analysis.counts.objects += 1,
            "array" => self.analysis.counts.arrays += 1,
            "variable_declarator" if parent_kind == "lexical_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.bind(name, LEXICAL);
                }
                let is_const = node
                    .parent()
                    .and_then(|p| p.child_by_field_name("kind"))
                    .is_some_and(|k| &self.source[k.byte_range()] == "const");
                if is_const && node.child_by_field_name("value").is_none() {
                    self.analysis.validity.initializers += 1;
                }
            }
            "assignment_expression" | "augmented_assignment_expression" | "update_expression" => {
                let field = if kind == "update_expression" {
                    "argument"
                } else {
                    "left"
                };
                if !node
                    .child_by_field_name(field)
                    .is_some_and(valid_assignment_target)
                {
                    self.analysis.validity.assignments += 1;
                }
            }
            "return_statement" if self.enclosing_loops.is_empty() => {
                self.analysis.validity.returns += 1;
            }
            "break_statement" | "continue_statement" if self.loops == 0 => {
                self.analysis.validity.loop_control += 1;
            }
            _ => {}
        }
    }

    fn leave(&mut self, node: Node<'_>) {
        let kind = node.kind();
        if (kind == "statement_block" && !is_function_body(node))
            || matches!(kind, "for_statement" | "for_in_statement")
        {
            self.scopes.pop();
        }
        if is_loop(kind) {
            self.loops -= 1;
        }
        if is_function(kind) {
            self.scopes.pop();
            self.loops = self.enclosing_loops.pop().expect("function entry");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(source: &str) -> Analysis {
        Validator::new().unwrap().analyze(source).unwrap()
    }

    #[test]
    fn rejects_recovery_trees_and_reuses_parser() {
        let mut validator = Validator::new().unwrap();
        for source in ["let = ;", "function f() {", "const a = [1,,;"] {
            assert!(
                validator.analyze(source).unwrap().validity.syntax > 0,
                "{source}"
            );
            assert!(validator.analyze("let a = 1;").unwrap().validity.is_valid());
        }
    }

    #[test]
    fn lexical_scopes_and_parameter_conflicts() {
        for source in [
            "let x; const x = 1;",
            "function f(x) { let x; }",
            "let f; function f() {}",
            "function f() {} const f = 1;",
            "for (let x = 0, x = 1; ; ) {}",
        ] {
            assert!(analyze(source).validity.bindings > 0, "{source}");
        }
        for source in [
            "let x; { const x = 1; }",
            "function f(x) { { let x; } }",
            "function f(x, x) { return x; }",
            "let x; for (let x = 0; x < 2; x++) { let x; }",
        ] {
            assert!(analyze(source).validity.is_valid(), "{source}");
        }
    }

    #[test]
    fn const_initializers_and_for_each_exceptions() {
        assert_eq!(analyze("const x;").validity.initializers, 1);
        assert_eq!(analyze("for (const x; ; ) {}").validity.initializers, 1);
        for source in [
            "const x = 1;",
            "for (const x of xs) {}",
            "for (const x in xs) {}",
        ] {
            assert!(analyze(source).validity.is_valid(), "{source}");
        }
    }

    #[test]
    fn control_flow_cannot_cross_function_boundaries() {
        assert_eq!(analyze("{ return 1; }").validity.returns, 1);
        assert_eq!(
            analyze("while (1) { function f() { break; continue; } }")
                .validity
                .loop_control,
            2
        );
        assert_eq!(analyze("function f() {} return 1;").validity.returns, 1);
        assert!(
            analyze("function f() { while (1) { break; } return 0; }")
                .validity
                .is_valid()
        );
        assert!(
            analyze("while (1) { function f() {} continue; }")
                .validity
                .is_valid()
        );
        assert_eq!(analyze("while (1) {} break;").validity.loop_control, 1);
    }

    #[test]
    fn assignment_targets_and_runtime_errors() {
        assert!(analyze("(1) = 2;").validity.assignments > 0);
        assert!(analyze("(f())++;").validity.assignments > 0);
        for source in [
            "(x) = 1; x.y++; x[0] += 1;",
            "missing();",
            "const x = 1; x = 2;",
        ] {
            assert!(analyze(source).validity.is_valid(), "{source}");
        }
    }

    #[test]
    fn counts_and_size_goals_are_independent_of_acceptance() {
        let a = analyze(
            "function f() { const x = {a: [1]}; for (let i = 0; i < 2; i++) { x.a; } return x; }",
        );
        assert!(a.validity.is_valid());
        assert_eq!(
            a.counts,
            Counts {
                functions: 1,
                statements: 5,
                objects: 1,
                arrays: 1,
                loops: 1
            }
        );
        assert_eq!(SizeGoals::default().penalties(a.counts), [0, 5, 0, 0, 0]);
        let tiny = analyze("let x;");
        assert!(tiny.validity.is_valid());
        assert_eq!(SizeGoals::default().penalties(tiny.counts), [0, 9, 1, 1, 1]);
    }
}
