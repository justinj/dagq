use crate::{
    physical::{BoxOperator, Forwards, Index},
    planner::Expr,
};
use vocab::Rev;

impl Expr {
    fn lower<'a>(self, index: &'a Index<Rev, Forwards>) -> BoxOperator<'a, Rev, Forwards> {
        match self {
            Expr::None => todo!(),
            Expr::All => todo!(),
            // Expr::Constant(items) => physical::Constant::new(items),
            Expr::Constant(items) => todo!(),
            Expr::Up { input, lo, hi } => todo!(),
            Expr::Down { input, lo, hi } => todo!(),
            Expr::Range { lo, hi } => todo!(),
            Expr::Union(exprs) => todo!(),
            Expr::Intersection(exprs) => todo!(),
            Expr::Filter { input, preds } => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lang, physical::Operator};

    use super::*;

    #[test]
    fn lowers_and_runs_constant_expression() {
        let index = Index::<Rev, Forwards>::new(vec![Rev(0), Rev(1), Rev(2), Rev(3)]);
        let expr = lang::parse_and_lower_to_planner("3").unwrap();

        let mut operator = expr.lower(&index);
        let actual: Vec<_> = operator.iter().collect();

        assert_eq!(actual, vec![Rev(3)]);
    }
}
