// TypeScript parser → HIR — stub for M1
pub mod checker;
use oxc::allocator::Allocator;
use oxc::parser::{ParseOptions, Parser};
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;

pub fn to_hir(source: &str) -> Result<(), &str> {
    let allocator = Allocator::default();
    let source_type = SourceType::default();
    let options = ParseOptions::default();
    let parser = Parser::new(&allocator, source, source_type);
    let program = parser.parse();
    let semantic = SemanticBuilder::new();

    Ok(())
}
