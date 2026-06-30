use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::fmt::{self, Write as _};

mod physical;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Vx(pub usize);

impl Vx {
    fn next(&self) -> Self {
        Vx(self.0 + 1)
    }
}

#[derive(Debug)]
pub struct Dag {
    root: Vx,
    edges: Relation<Vx>,
    rev_edges: Relation<Vx>,

    // TODO: special relation that stores all the strings in a big buffer.
    annotations: HashMap<String, Relation<String>>,
}

#[derive(Default)]
pub struct DagBuilder {
    last_vx: Vx,
    edges: Relation<Vx>,

    // TODO: special relation that stores all the strings in a big buffer.
    annotations: HashMap<String, Vec<(Vx, String)>>,
}

// Unary relation over revsets.
#[derive(Debug)]
pub struct Batch {
    // Always ordered.
    data: Vec<Vx>,
}

impl Batch {
    fn new(data: Vec<Vx>) -> Self {
        Batch { data }
    }

    fn iter(&self) -> impl Iterator<Item = Vx> {
        self.data.iter().copied()
    }

    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    fn extend(&mut self, d: impl Iterator<Item = Vx>) {
        self.data.extend(d)
    }

    fn sort_unstable(&mut self) {
        self.data.sort_unstable();
    }

    fn dedup(&mut self) {
        self.data.dedup();
    }

    fn filter<T: Ord + Clone>(&self, value: T, r: &Relation<T>) -> Batch {
        let mut res = Vec::new();
        let mut bi = 0;
        let mut ri = 0;

        while bi < self.data.len() && ri < r.data.len() {
            match self.data[bi].cmp(&r.data[ri].0) {
                Ordering::Less => bi += 1,
                Ordering::Greater => ri += 1,
                Ordering::Equal => {
                    let vx = self.data[bi];
                    let mut matched = false;
                    while ri < r.data.len() && r.data[ri].0 == vx {
                        matched |= r.data[ri].1 == value;
                        ri += 1;
                    }
                    if matched {
                        res.push(vx);
                    }
                    bi += 1;
                }
            }
        }

        Batch::new(res)
    }

    fn join<T: Ord + Clone>(&self, r: &Relation<T>) -> Vec<T> {
        let mut res = Vec::new();
        let mut bi = 0;
        let mut ri = 0;

        while bi < self.data.len() && ri < r.data.len() {
            match self.data[bi].cmp(&r.data[ri].0) {
                Ordering::Less => bi += 1,
                Ordering::Greater => ri += 1,
                Ordering::Equal => {
                    let vx = self.data[bi];
                    while ri < r.data.len() && r.data[ri].0 == vx {
                        res.push(r.data[ri].1.clone());
                        ri += 1;
                    }
                    bi += 1;
                }
            }
        }

        res.dedup();
        res
    }

    fn union_all(batches: Vec<Batch>) -> Batch {
        let mut heap = BinaryHeap::new();
        let mut cursors = vec![0; batches.len()];

        for (i, batch) in batches.iter().enumerate() {
            if let Some(&vx) = batch.data.first() {
                heap.push(Reverse((vx, i)));
            }
        }

        let mut res = Vec::new();
        while let Some(Reverse((vx, i))) = heap.pop() {
            if res.last() != Some(&vx) {
                res.push(vx);
            }

            cursors[i] += 1;
            if let Some(&next) = batches[i].data.get(cursors[i]) {
                heap.push(Reverse((next, i)));
            }
        }

        Batch::new(res)
    }

    fn intersection_all(batches: Vec<Batch>) -> Batch {
        if batches.is_empty() || batches.iter().any(Batch::is_empty) {
            return Batch::new(Vec::new());
        }

        let mut heap = BinaryHeap::new();
        let mut cursors = vec![0; batches.len()];
        let mut max = batches[0].data[0];

        for (i, batch) in batches.iter().enumerate() {
            let vx = batch.data[0];
            max = max.max(vx);
            heap.push(Reverse((vx, i)));
        }

        let mut res = Vec::new();
        while heap.len() == batches.len() {
            let Reverse((vx, i)) = heap.pop().unwrap();

            if vx == max {
                res.push(vx);
            }

            loop {
                cursors[i] += 1;
                let Some(&next) = batches[i].data.get(cursors[i]) else {
                    return Batch::new(res);
                };

                if next >= max {
                    max = max.max(next);
                    heap.push(Reverse((next, i)));
                    break;
                }
            }
        }

        Batch::new(res)
    }
}

#[derive(Debug, Default)]
pub struct Relation<T>
where
    T: Ord + Clone,
{
    // Always ordered on the first coordinate.
    data: Vec<(Vx, T)>,
}

impl<T> Relation<T>
where
    T: Ord + Clone,
{
    fn new(mut data: Vec<(Vx, T)>) -> Self {
        data.sort_unstable();
        data.dedup();
        Self { data }
    }
}

impl Dag {
    pub fn all(&self) -> Batch {
        Batch::new(self.rev_edges.data.iter().map(|&(f, _)| f).collect())
    }

    pub fn root(&self) -> Vx {
        self.root
    }

    fn iter_up(&self, vxs: Batch) -> Batch {
        Batch::new(vxs.join(&self.edges))
    }

    fn iter_down(&self, vxs: Batch) -> Batch {
        Batch::new(vxs.join(&self.rev_edges))
    }

    pub fn evaluator(&self) -> Evaluator<'_> {
        Evaluator { dag: self }
    }
}

impl DagBuilder {
    pub fn root(&self) -> Vx {
        Vx(0)
    }

    pub fn vx(&mut self) -> Vx {
        self.last_vx = self.last_vx.next();
        self.last_vx
    }

    pub fn edge(&mut self, from: Vx, to: Vx) {
        self.edges.data.push((from, to))
    }

    // TODO: bad allocation of label
    pub fn annotate(&mut self, vx: Vx, label: impl Into<String>, annotation: impl Into<String>) {
        self.annotations
            .entry(label.into())
            .or_default()
            .push((vx, annotation.into()))
    }

    pub fn build(mut self) -> Dag {
        self.edges.data.sort_unstable();

        let mut rev_edges = Relation::default();
        for &(from, to) in &self.edges.data {
            rev_edges.data.push((to, from));
        }
        rev_edges.data.sort_unstable();

        let mut annotations = HashMap::new();
        for (k, v) in self.annotations.drain() {
            annotations.insert(k, Relation::new(v));
        }

        Dag {
            root: self.root(),
            edges: self.edges,
            rev_edges,
            annotations,
        }
    }

    pub fn m(&mut self, vxs: impl IntoIterator<Item = Vx>) -> Vx {
        let c = self.vx();
        for vx in vxs {
            self.edge(vx, c);
        }
        c
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    All,
    Constant(Vec<Vx>),
    Up {
        input: Box<Expr>,
        // Only include results if they appeared in the lo-th round.
        lo: usize,
        // Don't include results appearing later than the hi-th round.
        hi: Option<usize>,
    },
    Down {
        input: Box<Expr>,
        // Only include results if they appeared in the lo-th round.
        lo: usize,
        // Don't include results appearing later than the hi-th round.
        hi: Option<usize>,
    },
    Range {
        lo: Box<Expr>,
        hi: Box<Expr>,
    },
    Union(Vec<Expr>),
    Intersection(Vec<Expr>),
    Filter {
        input: Box<Expr>,
        label: String,
        value: String,
    },
}

#[derive(Debug, Clone)]
pub struct Planner {
    pub rules: RewriteRules,
}

impl Default for Planner {
    fn default() -> Self {
        Self {
            rules: RewriteRules::all(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RewriteRules {
    pub fold_up_up: bool,
    pub fold_down_down: bool,
    pub flatten_union: bool,
    pub flatten_intersection: bool,
    pub intersection_to_filter: bool,
}

impl RewriteRules {
    pub fn all() -> Self {
        Self {
            fold_up_up: true,
            fold_down_down: true,
            flatten_union: true,
            flatten_intersection: true,
            intersection_to_filter: true,
        }
    }

    pub fn none() -> Self {
        Self {
            fold_up_up: false,
            fold_down_down: false,
            flatten_union: false,
            flatten_intersection: false,
            intersection_to_filter: false,
        }
    }
}

impl Planner {
    pub fn optimize(&self, expr: Expr) -> Expr {
        match expr {
            Expr::All => expr,
            Expr::Constant(_) => expr,
            Expr::Up { input, lo, hi } => {
                let input = self.optimize(*input);
                if self.rules.fold_up_up {
                    match input {
                        Expr::Up {
                            input,
                            lo: inner_lo,
                            hi: inner_hi,
                        } => {
                            let lo = inner_lo + lo;
                            let hi = match (inner_hi, hi) {
                                (Some(inner_hi), Some(hi)) => Some(inner_hi + hi),
                                _ => None,
                            };
                            Expr::Up { input, lo, hi }
                        }
                        input => Expr::Up {
                            input: Box::new(input),
                            lo,
                            hi,
                        },
                    }
                } else {
                    Expr::Up {
                        input: Box::new(input),
                        lo,
                        hi,
                    }
                }
            }
            Expr::Down { input, lo, hi } => {
                let input = self.optimize(*input);
                if self.rules.fold_down_down {
                    match input {
                        Expr::Down {
                            input,
                            lo: inner_lo,
                            hi: inner_hi,
                        } => {
                            let lo = inner_lo + lo;
                            let hi = match (inner_hi, hi) {
                                (Some(inner_hi), Some(hi)) => Some(inner_hi + hi),
                                _ => None,
                            };
                            Expr::Down { input, lo, hi }
                        }
                        input => Expr::Down {
                            input: Box::new(input),
                            lo,
                            hi,
                        },
                    }
                } else {
                    Expr::Down {
                        input: Box::new(input),
                        lo,
                        hi,
                    }
                }
            }
            Expr::Range { lo, hi } => Expr::Range {
                lo: Box::new(self.optimize(*lo)),
                hi: Box::new(self.optimize(*hi)),
            },
            Expr::Union(inputs) => {
                let inputs = inputs.into_iter().map(|input| self.optimize(input));
                if self.rules.flatten_union {
                    Expr::Union(
                        inputs
                            .flat_map(|input| match input {
                                Expr::Union(inputs) => inputs,
                                input => vec![input],
                            })
                            .collect(),
                    )
                } else {
                    Expr::Union(inputs.collect())
                }
            }
            Expr::Intersection(inputs) => {
                let mut inputs: Vec<_> = inputs
                    .into_iter()
                    .map(|input| self.optimize(input))
                    .collect();

                if inputs.len() == 0 {
                    return Expr::All;
                }

                if inputs.len() == 1 {
                    return inputs.remove(0);
                }

                if self.rules.flatten_intersection {
                    if inputs.iter().any(|i| matches!(i, Expr::Intersection(_))) {
                        return Expr::Intersection(
                            inputs
                                .into_iter()
                                .flat_map(|input| match input {
                                    Expr::Intersection(inputs) => inputs,
                                    input => vec![input],
                                })
                                .collect(),
                        )
                        .optimize(self);
                    }
                }

                if self.rules.intersection_to_filter {
                    if inputs.iter().any(|input| {
                        matches!(
                            input,
                            Expr::Filter { input, .. }
                                if matches!(input.as_ref(), Expr::All)
                        )
                    }) {
                        let (hoisted_filters, base_inputs): (Vec<_>, Vec<_>) =
                            inputs.into_iter().partition(|input| {
                                matches!(
                                    input,
                                    Expr::Filter { input, .. }
                                        if matches!(input.as_ref(), Expr::All)
                                )
                            });

                        let mut expr = Expr::Intersection(base_inputs).optimize(self);

                        for filter in hoisted_filters {
                            let Expr::Filter { label, value, .. } = filter else {
                                unreachable!();
                            };
                            expr = expr.filter(label, value);
                        }

                        return expr;
                    }
                }

                Expr::Intersection(inputs)
            }
            Expr::Filter {
                input,
                label,
                value,
            } => Expr::Filter {
                input: Box::new(self.optimize(*input)),
                label,
                value,
            },
        }
    }
}

pub struct Evaluator<'a> {
    dag: &'a Dag,
}

impl<'a> Evaluator<'a> {
    pub fn eval(&mut self, expr: Expr) -> Batch {
        match expr {
            Expr::All => self.dag.all(),
            Expr::Constant(mut vxs) => {
                vxs.sort_unstable();
                vxs.dedup();
                Batch::new(vxs)
            }
            Expr::Up { input, lo, hi } => {
                let mut frontier = self.eval(*input);
                let mut res = Batch::new(Vec::new());

                if lo == 0 {
                    res.extend(frontier.iter());
                }

                if let Some(hi) = hi {
                    for dist in 1..=hi {
                        frontier = self.dag.iter_up(frontier);
                        if dist >= lo {
                            res.extend(frontier.iter());
                        }
                    }
                } else {
                    let mut dist = 0;
                    while !frontier.is_empty() {
                        frontier = self.dag.iter_up(frontier);
                        dist += 1;
                        if dist >= lo {
                            res.extend(frontier.iter());
                        }
                    }
                }

                res.sort_unstable();
                res.dedup();
                res
            }
            Expr::Down { input, lo, hi } => {
                let mut frontier = self.eval(*input);
                let mut res = Batch::new(Vec::new());

                if lo == 0 {
                    res.extend(frontier.iter());
                }

                if let Some(hi) = hi {
                    for dist in 1..=hi {
                        frontier = self.dag.iter_down(frontier);
                        if dist >= lo {
                            res.extend(frontier.iter());
                        }
                    }
                } else {
                    let mut dist = 0;
                    while !frontier.is_empty() {
                        frontier = self.dag.iter_down(frontier);
                        dist += 1;
                        if dist >= lo {
                            res.extend(frontier.iter());
                        }
                    }
                }

                res.sort_unstable();
                res.dedup();
                res
            }
            Expr::Union(inputs) => {
                Batch::union_all(inputs.into_iter().map(|input| self.eval(input)).collect())
            }
            Expr::Intersection(inputs) => {
                Batch::intersection_all(inputs.into_iter().map(|input| self.eval(input)).collect())
            }
            Expr::Filter {
                input,
                label,
                value,
            } => {
                let input = self.eval(*input);
                let Some(relation) = self.dag.annotations.get(&label) else {
                    return Batch::new(Vec::new());
                };
                input.filter(value, relation)
            }
            Expr::Range { lo, hi } => {
                let lo = self.eval(*lo).data;
                let hi = self.eval(*hi).data;
                let descendants = self.eval(Expr::constant(lo).up(0, None));
                let ancestors = self.eval(Expr::constant(hi).down(0, None));
                Batch::intersection_all(vec![descendants, ancestors])
            }
        }
    }
}

fn fmt_hi(hi: Option<usize>) -> String {
    hi.map_or_else(|| "*".to_string(), |hi| hi.to_string())
}

pub struct ExprTree<'a>(&'a Expr);

impl fmt::Display for ExprTree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.chain_string(0))
    }
}

fn write_arg(out: &mut String, expr: &Expr, indent: usize) -> fmt::Result {
    let rendered = expr.chain_string(indent);
    let Some(rendered) = rendered.strip_suffix('\n') else {
        return Ok(());
    };
    let Some((prefix, last)) = rendered.rsplit_once('\n') else {
        return writeln!(out, "{},", rendered);
    };
    writeln!(out, "{}", prefix)?;
    writeln!(out, "{},", last)
}

impl Expr {
    pub fn tree(&self) -> ExprTree<'_> {
        ExprTree(self)
    }

    fn chain_string(&self, indent: usize) -> String {
        let mut out = String::new();
        self.write_chain(&mut out, indent).unwrap();
        out
    }

    fn write_chain(&self, out: &mut String, indent: usize) -> fmt::Result {
        let pad = "  ".repeat(indent);
        match self {
            Expr::All => writeln!(out, "{}all()", pad,),
            Expr::Constant(vxs) => writeln!(out, "{}constant({:?})", pad, vxs),
            Expr::Up { input, lo, hi } => {
                input.write_chain(out, indent)?;
                let method_pad = "  ".repeat(indent + 1);
                writeln!(out, "{}.up({}, {})", method_pad, lo, fmt_hi(*hi))
            }
            Expr::Down { input, lo, hi } => {
                input.write_chain(out, indent)?;
                let method_pad = "  ".repeat(indent + 1);
                writeln!(out, "{}.down({}, {})", method_pad, lo, fmt_hi(*hi))
            }
            Expr::Filter {
                input,
                label,
                value,
            } => {
                input.write_chain(out, indent)?;
                let method_pad = "  ".repeat(indent + 1);
                writeln!(out, "{}.filter({:?}, {:?})", method_pad, label, value)
            }
            Expr::Range { lo, hi } => {
                writeln!(out, "{}range(", pad)?;
                write_arg(out, lo, indent + 1)?;
                write_arg(out, hi, indent + 1)?;
                writeln!(out, "{})", pad)
            }
            Expr::Union(inputs) => self.write_call(out, indent, "union", inputs),
            Expr::Intersection(inputs) => self.write_call(out, indent, "intersection", inputs),
        }
    }

    fn write_call(
        &self,
        out: &mut String,
        indent: usize,
        name: &str,
        inputs: &[Expr],
    ) -> fmt::Result {
        let pad = "  ".repeat(indent);
        writeln!(out, "{}{}(", pad, name)?;
        for input in inputs {
            write_arg(out, input, indent + 1)?;
        }
        writeln!(out, "{})", pad)
    }

    pub fn constant(vxs: impl Into<Vec<Vx>>) -> Self {
        Expr::Constant(vxs.into())
    }

    pub fn up(self, lo: usize, hi: Option<usize>) -> Self {
        Expr::Up {
            input: Box::new(self),
            lo,
            hi,
        }
    }

    pub fn down(self, lo: usize, hi: Option<usize>) -> Self {
        Expr::Down {
            input: Box::new(self),
            lo,
            hi,
        }
    }

    pub fn union(self, other: Expr) -> Self {
        Expr::Union(vec![self, other])
    }

    pub fn union_all(inputs: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Union(inputs.into_iter().collect())
    }

    pub fn intersection(self, other: Expr) -> Self {
        Expr::Intersection(vec![self, other])
    }

    pub fn intersection_all(inputs: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Intersection(inputs.into_iter().collect())
    }

    pub fn filter(self, label: impl Into<String>, value: impl Into<String>) -> Self {
        Expr::Filter {
            input: Box::new(self),
            label: label.into(),
            value: value.into(),
        }
    }

    pub fn optimize(self, planner: &Planner) -> Self {
        planner.optimize(self)
    }

    pub fn range(lo: Expr, hi: Expr) -> Expr {
        Expr::Range {
            lo: Box::new(lo),
            hi: Box::new(hi),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_up_rewrites() {
        let expr = Expr::intersection_all([
            Expr::union_all([
                Expr::constant(vec![Vx(0)]).up(1, None),
                Expr::constant(vec![Vx(3), Vx(4)]).down(1, Some(2)),
            ]),
            Expr::constant(vec![Vx(0)]).up(0, None).filter("name", "f"),
        ]);

        insta::assert_snapshot!(expr.tree().to_string(), @r###"
        intersection(
          union(
            constant([Vx(0)])
              .up(1, *),
            constant([Vx(3), Vx(4)])
              .down(1, 2),
          ),
          constant([Vx(0)])
            .up(0, *)
            .filter("name", "f"),
        )
        "###);

        let planner = Planner::default();
        let expr = Expr::constant(vec![Vx(0)])
            .up(0, None)
            .up(0, None)
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"
        constant([Vx(0)])
          .up(0, *)
        ");

        let expr = Expr::constant(vec![Vx(0)])
            .up(1, Some(1))
            .up(0, Some(1))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"
        constant([Vx(0)])
          .up(1, 2)
        ");

        let expr = Expr::All;

        insta::assert_snapshot!(expr.tree().to_string(), @"all()");

        let planner = Planner {
            rules: RewriteRules {
                fold_up_up: false,
                ..RewriteRules::all()
            },
        };
        let expr = Expr::constant(vec![Vx(0)])
            .up(0, None)
            .up(0, None)
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"
        constant([Vx(0)])
          .up(0, *)
          .up(0, *)
        ");
    }

    #[test]
    fn range_between_two_revsets() {
        let mut builder = DagBuilder::default();

        // z -> a -> b -> c
        let z = builder.root();
        let a = builder.m([z]);
        let b = builder.m([a]);
        let c = builder.m([b]);
        let dag = builder.build();

        let query = Expr::range(Expr::constant(vec![a]), Expr::constant(vec![c]));

        let mut evaluator = dag.evaluator();
        assert_eq!(evaluator.eval(query).data, vec![a, b, c]);
    }

    #[test]
    fn it_works() {
        let mut builder = DagBuilder::default();

        // DAG used by these tests:
        //
        //          z
        //        /   \
        //       a     b
        //        \   /
        //          c
        //        /   \
        //       d     e
        //        \     \
        //         \     f
        //          \   /
        //            g
        let z = builder.root();
        let a = builder.m([z]);
        let b = builder.m([z]);
        let c = builder.m([a, b]);
        let d = builder.m([c]);
        let e = builder.m([c]);
        let f = builder.m([e]);
        let g = builder.m([d, f]);

        builder.annotate(c, "author", "Justin Jaffray");
        builder.annotate(d, "author", "Justin Jaffray");

        builder.annotate(z, "name", "z");
        builder.annotate(a, "name", "a");
        builder.annotate(b, "name", "b");
        builder.annotate(c, "name", "c");
        builder.annotate(d, "name", "d");
        builder.annotate(e, "name", "e");
        builder.annotate(f, "name", "f");
        builder.annotate(g, "name", "g");

        let dag = builder.build();

        let query = Expr::Constant(vec![c])
            .up(0, None)
            .intersection(Expr::All.filter("author", "Justin Jaffray"));

        let planner = Planner::default();

        let mut evaluator = dag.evaluator();

        println!("{}", query.tree().to_string());
        let query = query.optimize(&planner);
        println!("{}", query.tree().to_string());

        println!("{:?}", evaluator.eval(query));

        let cases = [
            (Expr::constant(vec![dag.root]).up(1, Some(1)), vec![a, b]),
            (Expr::constant(vec![z]).up(2, Some(2)), vec![c]),
            (Expr::constant(vec![d, f]).up(1, Some(1)), vec![g]),
            (
                Expr::constant(vec![z]).up(1, None),
                vec![a, b, c, d, e, f, g],
            ),
            (Expr::constant(vec![g]).down(1, Some(1)), vec![d, f]),
            (
                Expr::constant(vec![g]).down(1, None),
                vec![z, a, b, c, d, e, f],
            ),
            (
                Expr::union_all([
                    Expr::constant(vec![z]).up(1, Some(1)),
                    Expr::constant(vec![c]).up(1, Some(1)),
                    Expr::constant(vec![f]),
                ]),
                vec![a, b, d, e, f],
            ),
            (
                Expr::intersection_all([
                    Expr::constant(vec![c]).up(1, None),
                    Expr::constant(vec![g]).down(1, None),
                    Expr::constant(vec![a, d, e, f]),
                ]),
                vec![d, e, f],
            ),
            (
                Expr::constant(vec![z]).up(1, None).filter("name", "f"),
                vec![f],
            ),
            (
                Expr::constant(vec![z])
                    .up(1, None)
                    .filter("name", "missing"),
                vec![],
            ),
        ];

        let mut evaluator = dag.evaluator();

        for (query, expected) in cases {
            assert_eq!(evaluator.eval(query).data, expected);
        }
    }
}
