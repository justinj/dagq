// TODO: share Vx with Rev

use std::fmt;
use std::fmt::Write;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Vx(pub usize);

impl Vx {
    fn next(&self) -> Self {
        Vx(self.0 + 1)
    }
}

#[derive(Debug, Clone)]
pub enum Expr {
    None,
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

impl Expr {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Expr::All)
    }

    fn unwrap_constant(self) -> Vec<Vx> {
        if let Expr::Constant(v) = self {
            v
        } else {
            panic!("attempted to unwrap_constant non-constant")
        }
    }
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
            Expr::None => expr,
            Expr::All => expr,
            Expr::Constant(mut vs) => {
                if vs.is_empty() {
                    return Expr::None;
                }

                vs.sort_unstable();
                vs.dedup();
                Expr::Constant(vs)
            }
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
                let mut inputs: Vec<Expr> = inputs
                    .into_iter()
                    .map(|input| self.optimize(input))
                    .collect();

                if self.rules.flatten_union && inputs.iter().any(|m| matches!(m, Expr::Union(_))) {
                    return Expr::Union(
                        inputs
                            .into_iter()
                            .flat_map(|input| match input {
                                Expr::Union(inputs) => inputs,
                                input => vec![input],
                            })
                            .collect(),
                    )
                    .optimize(self);
                }

                if inputs.len() == 0 {
                    return Expr::None;
                }

                if inputs.len() == 1 {
                    return inputs[0].take();
                }

                // TODO: add fields for all of these
                if inputs.iter().any(|e| matches!(e, Expr::None)) {
                    inputs.retain(|i| !matches!(i, Expr::None));
                    return Expr::Union(inputs).optimize(self);
                }

                if inputs.iter().any(|e| matches!(e, Expr::All)) {
                    return Expr::All;
                }

                // TODO: figure out a cleaner way for this
                let cmp = |u| match u {
                    &Expr::Constant(_) => 0,
                    _ => 1,
                };
                if !inputs.is_sorted_by_key(cmp) {
                    inputs.sort_by_key(|u| match u {
                        &Expr::Constant(_) => 0,
                        _ => 1,
                    });
                    return Expr::Union(inputs).optimize(self);
                }

                if inputs.len() > 1 && matches!(inputs[1], Expr::Constant(_)) {
                    let mut result = inputs[0].take().unwrap_constant();
                    let mut i = 1;
                    while i < inputs.len()
                        && let Expr::Constant(vs) = &inputs[i]
                    {
                        i += 1;
                        result.extend(vs);
                    }
                    let mut new_union = vec![Expr::Constant(result)];
                    new_union.extend((i..inputs.len()).map(|i| inputs[i].take()));
                    return Expr::Union(new_union).optimize(self);
                }

                Expr::Union(inputs)
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
            Expr::None => writeln!(out, "{}none()", pad,),
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
    fn test_rewrites() {
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

        let planner = Planner::default();
        let expr = Expr::constant(vec![Vx(0)])
            .union(Expr::constant(vec![Vx(1)]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"constant([Vx(0), Vx(1)])");

        let planner = Planner::default();
        let expr = Expr::constant(vec![Vx(1)])
            .union(Expr::constant(vec![Vx(0)]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"constant([Vx(0), Vx(1)])");

        let planner = Planner::default();
        let expr = Expr::constant(vec![])
            .union(Expr::constant(vec![]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"none()");
    }
}
