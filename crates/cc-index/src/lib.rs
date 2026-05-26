pub mod community;
pub mod config_linker;
pub mod dispatch_synthesis;
pub mod framework_registry;
pub mod framework_resolvers;
pub mod git_cochange;
pub mod hierarchy;
pub mod indexer;
pub mod infra_pass;
pub(crate) mod memory_budget;
pub mod resolver;
pub mod scanner;
pub mod type_catalog;

pub use framework_registry::FileFrameworkDetection;
pub use indexer::{IndexReport, Indexer};
pub use scanner::{ScannedFile, Scanner};
