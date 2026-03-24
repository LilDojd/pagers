use crate::mincore::PageMap;

use super::{FileContext, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Query;

impl Op for Query {
    const LABEL: &str = "resident";
    const MUTATES_RESIDENCY: bool = false;
    type Output = ();

    fn execute<PM: PageMap + Sync>(&self, _ctx: &FileContext<'_, PM>) -> crate::Result<()> {
        Ok(())
    }
}
