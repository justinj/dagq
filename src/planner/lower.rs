use crate::{
    physical::{BoxOperator, Constant, Forwards, Index, Union},
    planner::Expr,
};
use vocab::Rev;

impl Expr {
    fn lower<'a>(self, index: &'a Index<Rev, Forwards>) -> BoxOperator<'a, Rev, Forwards> {
        match self {
            Expr::None => todo!(),
            Expr::All => todo!(),
            // Expr::Constant(items) => physical::Constant::new(items),
            Expr::Constant(items) => Box::new(Constant::new(items)),
            Expr::Up { input, lo, hi } => todo!(),
            Expr::Down { input, lo, hi } => todo!(),
            Expr::Range { lo, hi } => todo!(),
            Expr::Union(exprs) => {
                // TODO: variadic physical union
                exprs
                    .into_iter()
                    .fold(Box::new(Constant::new(Vec::new())), |o, n| {
                        Box::new(Union::new(o, n.lower(index)))
                    })
            }
            Expr::Intersection(exprs) => todo!(),
            Expr::Filter { input, preds } => todo!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{lang, physical::Operator, planner::Planner};

    use super::*;

    fn run(expr: &str, index: &Index<Rev, Forwards>) -> Vec<Rev> {
        let expr = lang::parse_and_lower_to_planner(expr).unwrap();
        let planner = Planner::default();
        let expr = expr.optimize(&planner);
        let mut operator = expr.lower(&index);
        operator.iter().collect()
    }

    #[test]
    fn lowers_and_runs() {
        let index = Index::<Rev, Forwards>::new(vec![Rev(0), Rev(1), Rev(2), Rev(3)]);

        assert_eq!(run("3", &index), vec![Rev(3)]);
        assert_eq!(run("3|2", &index), vec![Rev(3), Rev(2)]);
    }
}
