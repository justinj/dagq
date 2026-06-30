use std::collections::HashMap;

use crate::lang::RevsetExpr;
use crate::physical::{Backwards, BoxOperator, Constant, Forwards, Index, Operator, Rev};

pub(super) struct Indexes {
    names: HashMap<String, Rev>,
    ancestors: Index<(Rev, Rev), Forwards>,
    descendants: Index<(Rev, Rev), Backwards>,
}

impl Indexes {
    pub(super) fn new(
        names: impl IntoIterator<Item = (impl Into<String>, Rev)>,
        child_parent_edges: Vec<(Rev, Rev)>,
    ) -> Self {
        let parent_child_edges: Vec<_> = child_parent_edges
            .iter()
            .map(|&(child, parent)| (parent, child))
            .collect();

        Self {
            names: names
                .into_iter()
                .map(|(name, rev)| (name.into(), rev))
                .collect(),
            ancestors: Index::new(child_parent_edges),
            descendants: Index::new(parent_child_edges),
        }
    }
}

pub(super) fn render<'a>(ast: &RevsetExpr, indexes: &'a Indexes) -> BoxOperator<'a, Rev, Forwards> {
    match ast {
        RevsetExpr::Literal(name) => {
            let rev = indexes
                .names
                .get(name)
                .copied()
                .unwrap_or_else(|| panic!("unknown revision literal: {name}"));
            Box::new(Constant::<_, Forwards>::new(vec![rev]))
        }
        RevsetExpr::Ancestors(input) => Box::new(render(input, indexes).ancestors(&indexes.ancestors)),
        RevsetExpr::Descendants(input) => Box::new(
            render(input, indexes)
                .reverse()
                .descendants(&indexes.descendants)
                .reverse(),
        ),
        RevsetExpr::Range(x, y) => Box::new(render(x, indexes).range(
            render(y, indexes),
            &indexes.descendants,
            &indexes.ancestors,
        )),
        RevsetExpr::Union(left, right) => Box::new(render(left, indexes).union(render(right, indexes))),
        RevsetExpr::Intersection(left, right) => Box::new(
            render(left, indexes).intersection(render(right, indexes)),
        ),
        RevsetExpr::Difference(left, right) => {
            Box::new(render(left, indexes).difference(render(right, indexes)))
        }
        RevsetExpr::Function { name, .. } => panic!("unknown revset function: {name}"),
    }
}

pub(super) fn run(ast: &RevsetExpr, indexes: &Indexes) -> Vec<Rev> {
    render(ast, indexes).iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;

    #[test]
    fn renders_revset_ast_to_physical_plan_and_results() {
        // Graph:
        //
        //      0
        //     / \
        //    1   2
        //     \ /
        //      3
        //     / \
        //    4   5
        //         \
        //          6
        let indexes = Indexes::new(
            (0..=6).map(|i| (i.to_string(), Rev(i))),
            vec![
                (Rev(1), Rev(0)),
                (Rev(2), Rev(0)),
                (Rev(3), Rev(1)),
                (Rev(3), Rev(2)),
                (Rev(4), Rev(3)),
                (Rev(5), Rev(3)),
                (Rev(6), Rev(5)),
            ],
        );

        let cases = [
            ("1 | 2", vec![Rev(2), Rev(1)]),
            ("::6", vec![Rev(5), Rev(3), Rev(2), Rev(1), Rev(0)]),
            ("0::", vec![Rev(6), Rev(5), Rev(4), Rev(3), Rev(2), Rev(1)]),
            ("3::6", vec![Rev(6), Rev(5), Rev(3)]),
            ("(1 | 4) | (2 | 4)", vec![Rev(4), Rev(2), Rev(1)]),
            ("0:: & (2 | 4 | 6)", vec![Rev(6), Rev(4), Rev(2)]),
            ("0:: ~ (2 | 5)", vec![Rev(6), Rev(4), Rev(3), Rev(1)]),
        ];

        for (query, expected) in cases {
            let ast = parse(query).unwrap();
            assert_eq!(run(&ast, &indexes), expected, "query: {query}");
        }
    }
}
