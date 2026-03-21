use super::{FileContext, Op};

pub struct Query;

impl Op for Query {
    const LABEL: &str = "resident";
    type Output = ();

    fn execute(&self, _ctx: &FileContext) -> crate::Result<()> {
        Ok(())
    }
}
