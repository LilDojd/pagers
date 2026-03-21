use super::{FileContext, Op};

pub struct Query;

impl Op for Query {
    type Output = ();

    fn execute(&self, _ctx: &FileContext) -> crate::Result<()> {
        Ok(())
    }
}
