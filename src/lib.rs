use std::cmp::Ordering;

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
}

#[derive(Default)]
pub struct DagBuilder {
    last_vx: Vx,
    edges: Relation<Vx>,
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
}

#[derive(Debug, Default)]
pub struct Relation<T>
where
    T: Ord + Clone,
{
    // Always ordered on the first coordinate.
    data: Vec<(Vx, T)>,
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

    pub fn build(mut self) -> Dag {
        self.edges.data.sort_unstable();

        let mut rev_edges = Relation::default();
        for &(from, to) in &self.edges.data {
            rev_edges.data.push((to, from));
        }
        rev_edges.data.sort_unstable();

        Dag {
            root: self.root(),
            edges: self.edges,
            rev_edges,
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
        }
    }
}

impl Expr {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ];

        let mut evaluator = dag.evaluator();

        for (query, expected) in cases {
            assert_eq!(evaluator.eval(query).data, expected);
        }

        println!("{:#?}", dag);
    }
}
