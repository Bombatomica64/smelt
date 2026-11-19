//! The emitter's side of `crate::list_links`: when a list conversion links
//! rather than copies.

use super::*;
use crate::rust::RustExpr;

impl FunctionEmitter<'_> {
    /// Whether a list of `source_item`s converted to a list of `target_item`s
    /// is linked to it (`crate::list_links`): the program carries the link,
    /// the target element is `unknown`, and each source element round-trips
    /// through the erased carrier (the live view's rule,
    /// [`Self::list_erases_live`]).
    pub(super) fn links_list_to_unknown(&self, source_item: TypeId, target_item: TypeId) -> bool {
        self.context.demand().has(Need::ListLinks)
            && crate::list_links::links_elements(self.mir, source_item, target_item)
            && self.list_erases_live(source_item)
    }

    /// The statement appending `item` (bound first, see the push emitters) to
    /// the list `list`, whose mutable borrow is `list_mut`: the buffer's own
    /// `push`, or, in a program that links typed lists to `unknown[]` views
    /// (`crate::list_links`), `push_item`, which keeps a linked view in step
    /// by that one element.
    pub(super) fn push_statement(
        &self,
        list: &RustExpr,
        list_mut: &RustExpr,
        item: &RustExpr,
    ) -> String {
        if self.context.demand().has(Need::ListLinks) {
            format!(
                "let smelt_push_item = {item}; {list}.push_item(smelt_push_item);",
                item = item.delimited(),
                list = list.tight()
            )
        } else {
            format!(
                "let smelt_push_item = {item}; {list_mut}.push(smelt_push_item);",
                item = item.delimited(),
                list_mut = list_mut.tight()
            )
        }
    }
}
