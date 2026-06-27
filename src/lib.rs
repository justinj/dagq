use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::fmt;

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

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
    Constant(Vec<Vx>),
    Up {
        input: Box<Expr>,
        lo: usize,
        hi: Option<usize>,
    },
    Down {
        input: Box<Expr>,
        lo: usize,
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

pub struct Evaluator<'a> {
    dag: &'a Dag,
}

impl<'a> Evaluator<'a> {
    pub fn eval(&mut self, expr: Expr) -> Batch {
        match expr {
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
            Expr::Range { lo: _, hi: _ } => unimplemented!(),
        }
    }
}

fn fmt_hi(hi: Option<usize>) -> String {
    hi.map_or_else(|| "*".to_string(), |hi| hi.to_string())
}

pub struct ExprTree<'a>(&'a Expr);

impl fmt::Display for ExprTree<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt_tree(f, "", true, true)
    }
}

impl Expr {
    pub fn tree(&self) -> ExprTree<'_> {
        ExprTree(self)
    }

    fn fmt_tree(
        &self,
        f: &mut fmt::Formatter<'_>,
        prefix: &str,
        is_last: bool,
        is_root: bool,
    ) -> fmt::Result {
        if is_root {
            writeln!(f, "{}", self.tree_label())?;
        } else {
            writeln!(
                f,
                "{}{}{}",
                prefix,
                if is_last { "└── " } else { "├── " },
                self.tree_label()
            )?;
        }

        let child_prefix = if is_root {
            String::new()
        } else {
            format!("{}{}", prefix, if is_last { "    " } else { "│   " })
        };
        let children = self.children();
        for (idx, child) in children.iter().enumerate() {
            child.fmt_tree(f, &child_prefix, idx + 1 == children.len(), false)?;
        }
        Ok(())
    }

    fn tree_label(&self) -> String {
        match self {
            Expr::Constant(vxs) => format!("Constant {:?}", vxs),
            Expr::Up { lo, hi, .. } => format!("Up lo={} hi={}", lo, fmt_hi(*hi)),
            Expr::Down { lo, hi, .. } => format!("Down lo={} hi={}", lo, fmt_hi(*hi)),
            Expr::Range { .. } => "Range".to_string(),
            Expr::Union(_) => "Union".to_string(),
            Expr::Intersection(_) => "Intersection".to_string(),
            Expr::Filter { label, value, .. } => format!("Filter {}={:?}", label, value),
        }
    }

    fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Constant(_) => Vec::new(),
            Expr::Up { input, .. } | Expr::Down { input, .. } | Expr::Filter { input, .. } => {
                vec![input.as_ref()]
            }
            Expr::Range { lo, hi } => vec![lo.as_ref(), hi.as_ref()],
            Expr::Union(inputs) | Expr::Intersection(inputs) => inputs.iter().collect(),
        }
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
        match self {
            Expr::Union(mut inputs) => {
                inputs.push(other);
                Expr::Union(inputs)
            }
            expr => Expr::Union(vec![expr, other]),
        }
    }

    pub fn union_all(inputs: impl IntoIterator<Item = Expr>) -> Self {
        Expr::Union(inputs.into_iter().collect())
    }

    pub fn intersection(self, other: Expr) -> Self {
        match self {
            Expr::Intersection(mut inputs) => {
                inputs.push(other);
                Expr::Intersection(inputs)
            }
            expr => Expr::Intersection(vec![expr, other]),
        }
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

    pub fn range(lo: Expr, hi: Expr) -> Expr {
        lo.up(0, None).intersection(hi.down(0, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expr_shape_snapshot() {
        let expr = Expr::intersection_all([
            Expr::union_all([
                Expr::constant(vec![Vx(0)]).up(1, None),
                Expr::constant(vec![Vx(3), Vx(4)]).down(1, Some(2)),
            ]),
            Expr::constant(vec![Vx(0)]).up(0, None).filter("name", "f"),
        ]);

        insta::assert_snapshot!(expr.tree().to_string(), @r###"
        Intersection
        ├── Union
        │   ├── Up lo=1 hi=*
        │   │   └── Constant [Vx(0)]
        │   └── Down lo=1 hi=2
        │       └── Constant [Vx(3), Vx(4)]
        └── Filter name="f"
            └── Up lo=0 hi=*
                └── Constant [Vx(0)]
        "###);
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
        builder.annotate(z, "name", "z");
        let a = builder.m([z]);
        builder.annotate(a, "name", "a");
        let b = builder.m([z]);
        builder.annotate(b, "name", "b");
        let c = builder.m([a, b]);
        builder.annotate(c, "name", "c");
        let d = builder.m([c]);
        builder.annotate(d, "name", "d");
        let e = builder.m([c]);
        builder.annotate(e, "name", "e");
        let f = builder.m([e]);
        builder.annotate(f, "name", "f");
        let g = builder.m([d, f]);
        builder.annotate(g, "name", "g");

        let dag = builder.build();

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
