use super::{FileContext, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Query;

impl Op for Query {
    const LABEL: &str = "resident";
    type Output = ();

    fn execute(&self, _ctx: &FileContext) -> crate::Result<()> {
        Ok(())
    }
}
