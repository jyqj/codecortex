use serde::{Deserialize, Serialize};

/// A local variable type assignment record, capturing the inferred type
/// of a variable from its declaration or initialization expression.
///
/// This is an in-memory intermediate structure used during indexing to feed
/// into the TypeCatalog for improved method dispatch resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAssignRecord {
    /// File where the assignment occurs.
    pub file_path: String,
    /// UID of the enclosing function/method, if any.
    pub enclosing_symbol_uid: Option<String>,
    /// Name of the variable being assigned.
    pub var_name: String,
    /// Inferred type name (e.g. "Foo", "http.Client").
    pub type_name: String,
    /// Source line number (1-based).
    pub line: u32,
    /// Confidence of the type inference.
    pub confidence: f64,
    /// How the type was inferred.
    pub source: TypeAssignSource,
}

/// How a variable's type was inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeAssignSource {
    /// `x = new Foo()` / `Foo{}` / `Foo::new()`
    Constructor,
    /// `x = createFoo()` / `NewFoo()`
    FactoryCall,
    /// `var x: Foo` / `Foo x = ...` / `x: Foo = ...`
    TypeAnnotation,
    /// `x = existing_foo_var` (propagation from another known variable)
    Assignment,
}
