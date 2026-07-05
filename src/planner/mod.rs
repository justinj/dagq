// TODO: share Vx with Rev

use std::fmt;
use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;

use jj_lib::object_id::ObjectId;
use jj_lib::revset::{
    RevsetCommitRef, RevsetExpression, RevsetFilterPredicate, UserRevsetExpression,
};
use jj_lib::str_util::{StringExpression, StringPattern};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Vx(pub usize);

impl Vx {
    fn next(&self) -> Self {
        Vx(self.0 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    Author(String),
    Description(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerRevsetError {
    InvalidLiteral(String),
    UnsupportedExpression(&'static str),
    UnsupportedCommitRef(String),
    UnsupportedFilter(&'static str),
    UnsupportedStringPattern(String),
    GenerationOutOfRange(Range<u64>),
}

impl fmt::Display for LowerRevsetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LowerRevsetError::InvalidLiteral(literal) => write!(f, "invalid literal {literal:?}"),
            LowerRevsetError::UnsupportedExpression(kind) => {
                write!(f, "unsupported jj revset expression {kind}")
            }
            LowerRevsetError::UnsupportedCommitRef(commit_ref) => {
                write!(f, "unsupported jj revset commit ref {commit_ref}")
            }
            LowerRevsetError::UnsupportedFilter(filter) => {
                write!(f, "unsupported jj revset filter {filter}")
            }
            LowerRevsetError::UnsupportedStringPattern(pattern) => {
                write!(f, "unsupported jj revset string pattern {pattern}")
            }
            LowerRevsetError::GenerationOutOfRange(range) => {
                write!(
                    f,
                    "jj revset generation range {range:?} does not fit planner IR"
                )
            }
        }
    }
}

impl std::error::Error for LowerRevsetError {}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        preds: Vec<Predicate>,
    },
}

pub fn lower_jj_revset(expr: &Arc<UserRevsetExpression>) -> Result<Expr, LowerRevsetError> {
    lower_jj_expr(expr)
}

fn lower_jj_expr(expr: &Arc<UserRevsetExpression>) -> Result<Expr, LowerRevsetError> {
    if let Some(predicate) = author_union_predicate(expr)? {
        return Ok(Expr::All.filter(vec![predicate]));
    }

    match expr.as_ref() {
        RevsetExpression::None => Ok(Expr::None),
        RevsetExpression::All => Ok(Expr::All),
        RevsetExpression::Commits(ids) => ids
            .iter()
            .map(|id| {
                id.hex()
                    .parse::<usize>()
                    .map(Vx)
                    .map_err(|_| LowerRevsetError::InvalidLiteral(id.hex()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Expr::constant),
        RevsetExpression::CommitRef(commit_ref) => lower_commit_ref(commit_ref),
        RevsetExpression::Ancestors {
            heads, generation, ..
        } => {
            let (lo, hi) = lower_generation_range(generation)?;
            Ok(lower_jj_expr(heads)?.down(lo, hi))
        }
        RevsetExpression::Descendants { roots, generation } => {
            let (lo, hi) = lower_generation_range(generation)?;
            Ok(lower_jj_expr(roots)?.up(lo, hi))
        }
        RevsetExpression::DagRange { roots, heads } => {
            Ok(Expr::range(lower_jj_expr(roots)?, lower_jj_expr(heads)?))
        }
        RevsetExpression::Range { roots, heads, .. } => {
            Ok(Expr::range(lower_jj_expr(roots)?, lower_jj_expr(heads)?))
        }
        RevsetExpression::Union(lhs, rhs) => Ok(lower_jj_expr(lhs)?.union(lower_jj_expr(rhs)?)),
        RevsetExpression::Intersection(lhs, rhs) => {
            Ok(lower_jj_expr(lhs)?.intersection(lower_jj_expr(rhs)?))
        }
        RevsetExpression::Difference(_, _) => {
            Err(LowerRevsetError::UnsupportedExpression("difference"))
        }
        RevsetExpression::Filter(predicate) => Ok(Expr::All.filter(vec![lower_filter(predicate)?])),
        _ => Err(LowerRevsetError::UnsupportedExpression(expr_kind(expr))),
    }
}

fn lower_commit_ref(commit_ref: &RevsetCommitRef) -> Result<Expr, LowerRevsetError> {
    match commit_ref {
        RevsetCommitRef::Symbol(symbol) => symbol
            .parse::<usize>()
            .map(|vx| Expr::constant(vec![Vx(vx)]))
            .map_err(|_| LowerRevsetError::InvalidLiteral(symbol.clone())),
        _ => Err(LowerRevsetError::UnsupportedCommitRef(format!(
            "{commit_ref:?}"
        ))),
    }
}

fn lower_generation_range(range: &Range<u64>) -> Result<(usize, Option<usize>), LowerRevsetError> {
    let lo = usize::try_from(range.start)
        .map_err(|_| LowerRevsetError::GenerationOutOfRange(range.clone()))?;
    let hi = if range.end == u64::MAX {
        None
    } else {
        let last = range
            .end
            .checked_sub(1)
            .ok_or_else(|| LowerRevsetError::GenerationOutOfRange(range.clone()))?;
        Some(
            usize::try_from(last)
                .map_err(|_| LowerRevsetError::GenerationOutOfRange(range.clone()))?,
        )
    };
    Ok((lo, hi))
}

fn lower_filter(predicate: &RevsetFilterPredicate) -> Result<Predicate, LowerRevsetError> {
    match predicate {
        RevsetFilterPredicate::Description(expr) => Ok(Predicate::Description(lower_string(expr)?)),
        RevsetFilterPredicate::AuthorName(expr) | RevsetFilterPredicate::AuthorEmail(expr) => {
            Ok(Predicate::Author(lower_string(expr)?))
        }
        _ => Err(LowerRevsetError::UnsupportedFilter(filter_kind(predicate))),
    }
}

fn lower_string(expr: &StringExpression) -> Result<String, LowerRevsetError> {
    match expr {
        StringExpression::Pattern(pattern) => match pattern.as_ref() {
            StringPattern::Exact(s)
            | StringPattern::ExactI(s)
            | StringPattern::Substring(s)
            | StringPattern::SubstringI(s) => Ok(s.clone()),
            pattern => Err(LowerRevsetError::UnsupportedStringPattern(format!(
                "{pattern:?}"
            ))),
        },
        expr => Err(LowerRevsetError::UnsupportedStringPattern(format!(
            "{expr:?}"
        ))),
    }
}

fn author_union_predicate(
    expr: &Arc<UserRevsetExpression>,
) -> Result<Option<Predicate>, LowerRevsetError> {
    let RevsetExpression::Union(lhs, rhs) = expr.as_ref() else {
        return Ok(None);
    };
    let (Some(lhs), Some(rhs)) = (author_filter(lhs)?, author_filter(rhs)?) else {
        return Ok(None);
    };
    if lhs == rhs { Ok(Some(lhs)) } else { Ok(None) }
}

fn author_filter(expr: &Arc<UserRevsetExpression>) -> Result<Option<Predicate>, LowerRevsetError> {
    let RevsetExpression::Filter(
        RevsetFilterPredicate::AuthorName(string) | RevsetFilterPredicate::AuthorEmail(string),
    ) = expr.as_ref()
    else {
        return Ok(None);
    };
    Ok(Some(Predicate::Author(lower_string(string)?)))
}

fn expr_kind(expr: &Arc<UserRevsetExpression>) -> &'static str {
    match expr.as_ref() {
        RevsetExpression::VisibleHeads => "visible_heads",
        RevsetExpression::VisibleHeadsOrReferenced => "visible_heads_or_referenced",
        RevsetExpression::Root => "root",
        RevsetExpression::Reachable { .. } => "reachable",
        RevsetExpression::Heads(_) => "heads",
        RevsetExpression::HeadsRange { .. } => "heads_range",
        RevsetExpression::Roots(_) => "roots",
        RevsetExpression::ForkPoint(_) => "fork_point",
        RevsetExpression::Forks => "forks",
        RevsetExpression::Bisect(_) => "bisect",
        RevsetExpression::HasSize { .. } => "has_size",
        RevsetExpression::Latest { .. } => "latest",
        RevsetExpression::AsFilter(_) => "as_filter",
        RevsetExpression::Divergent => "divergent",
        RevsetExpression::AtOperation { .. } => "at_operation",
        RevsetExpression::WithinReference { .. } => "within_reference",
        RevsetExpression::WithinVisibility { .. } => "within_visibility",
        RevsetExpression::Coalesce(_, _) => "coalesce",
        RevsetExpression::Present(_) => "present",
        RevsetExpression::NotIn(_) => "not_in",
        RevsetExpression::None
        | RevsetExpression::All
        | RevsetExpression::Commits(_)
        | RevsetExpression::CommitRef(_)
        | RevsetExpression::Ancestors { .. }
        | RevsetExpression::Descendants { .. }
        | RevsetExpression::Range { .. }
        | RevsetExpression::DagRange { .. }
        | RevsetExpression::Filter(_)
        | RevsetExpression::Union(_, _)
        | RevsetExpression::Intersection(_, _)
        | RevsetExpression::Difference(_, _) => "supported",
    }
}

fn filter_kind(predicate: &RevsetFilterPredicate) -> &'static str {
    match predicate {
        RevsetFilterPredicate::ParentCount(_) => "parent_count",
        RevsetFilterPredicate::Description(_) => "description",
        RevsetFilterPredicate::Subject(_) => "subject",
        RevsetFilterPredicate::AuthorName(_) => "author_name",
        RevsetFilterPredicate::AuthorEmail(_) => "author_email",
        RevsetFilterPredicate::AuthorDate(_) => "author_date",
        RevsetFilterPredicate::CommitterName(_) => "committer_name",
        RevsetFilterPredicate::CommitterEmail(_) => "committer_email",
        RevsetFilterPredicate::CommitterDate(_) => "committer_date",
        RevsetFilterPredicate::File(_) => "file",
        RevsetFilterPredicate::DiffLines { .. } => "diff_lines",
        RevsetFilterPredicate::HasConflict => "has_conflict",
        RevsetFilterPredicate::Signed => "signed",
        RevsetFilterPredicate::Extension(_) => "extension",
    }
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

    fn unwrap_up(self) -> (Box<Expr>, usize, Option<usize>) {
        if let Expr::Up { input, lo, hi } = self {
            (input, lo, hi)
        } else {
            panic!("attempted to unwrap_up non-up")
        }
    }

    fn unwrap_down(self) -> (Box<Expr>, usize, Option<usize>) {
        if let Expr::Down { input, lo, hi } = self {
            (input, lo, hi)
        } else {
            panic!("attempted to unwrap_down non-down")
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
}

impl RewriteRules {
    pub fn all() -> Self {
        Self {
            fold_up_up: true,
            fold_down_down: true,
            flatten_union: true,
            flatten_intersection: true,
        }
    }

    pub fn none() -> Self {
        Self {
            fold_up_up: false,
            fold_down_down: false,
            flatten_union: false,
            flatten_intersection: false,
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

                // Pull up matching filters
                {
                    let mut intersection: Option<Vec<Predicate>> = None;
                    for input in &inputs {
                        if let Expr::Filter { preds, .. } = input {
                            match &mut intersection {
                                Some(intersection) => intersection.retain(|p| preds.contains(p)),
                                None => intersection = Some(preds.clone()),
                            }
                        } else {
                            intersection = Some(Vec::new());
                        }
                    }

                    if let Some(intersection) = intersection
                        && !intersection.is_empty()
                    {
                        // There's a filter that every arm shares, so let's yank it up.
                        inputs = inputs
                            .into_iter()
                            .map(|e| {
                                if let Expr::Filter { input, mut preds } = e {
                                    preds.retain(|p| !intersection.contains(p));
                                    input.filter(preds)
                                } else {
                                    unreachable!()
                                }
                            })
                            .collect();
                        return Expr::Union(inputs).filter(intersection).optimize(self);
                    }
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

                if inputs.iter().any(|e| matches!(e, Expr::None)) {
                    return Expr::None;
                }

                let before_len = inputs.len();
                inputs.retain(|e| !matches!(e, Expr::All));
                if before_len != inputs.len() {
                    return Expr::Intersection(inputs).optimize(self);
                }

                // TODO: figure out a cleaner way for this
                let cmp = |u| match u {
                    &Expr::Constant(_) => 0,
                    &Expr::Up { .. } => 1,
                    &Expr::Down { .. } => 2,
                    _ => i32::MAX,
                };
                if !inputs.is_sorted_by_key(cmp) {
                    inputs.sort_by_key(|u| match u {
                        &Expr::Constant(_) => 0,
                        &Expr::Up { .. } => 1,
                        &Expr::Down { .. } => 2,
                        _ => i32::MAX,
                    });
                    return Expr::Intersection(inputs).optimize(self);
                }

                if inputs.len() > 1 && matches!(inputs[1], Expr::Constant(_)) {
                    let mut result = inputs[0].take().unwrap_constant();
                    let mut i = 1;
                    while i < inputs.len()
                        && let Expr::Constant(vs) = &inputs[i]
                    {
                        i += 1;
                        // TODO: hashset?
                        result.retain(|v| vs.contains(v))
                    }
                    let mut new_intersection = vec![Expr::Constant(result)];
                    new_intersection.extend((i..inputs.len()).map(|i| inputs[i].take()));
                    return Expr::Intersection(new_intersection).optimize(self);
                }

                // Merge a single up and a single down into a range.
                {
                    let mut ups = 0;
                    let mut downs = 0;
                    for e in &inputs {
                        match e {
                            Expr::Up {
                                lo: 0, hi: None, ..
                            } => ups += 1,
                            Expr::Down {
                                lo: 0, hi: None, ..
                            } => downs += 1,
                            _ => {}
                        }
                    }

                    if ups == 1 && downs == 1 {
                        let (mut ud, mut inputs): (Vec<_>, _) = inputs
                            .into_iter()
                            .partition(|e| matches!(e, Expr::Up { .. } | Expr::Down { .. }));
                        let (mut up, mut down) = (ud[0].take(), ud[1].take());
                        if matches!(up, Expr::Down { .. }) {
                            (up, down) = (down, up)
                        }
                        inputs.push(Expr::Range {
                            lo: up.unwrap_up().0,
                            hi: down.unwrap_down().0,
                        });
                        return Expr::Intersection(inputs).optimize(self);
                    }
                }

                {
                    let mut yanked_preds = Vec::new();
                    for i in 0..inputs.len() {
                        if let Expr::Filter { .. } = &inputs[i] {
                            let Expr::Filter { preds, input } = inputs[i].take() else {
                                unreachable!();
                            };
                            inputs[i] = *input;
                            yanked_preds.extend(preds);
                        }
                        if !yanked_preds.is_empty() {
                            return Expr::Intersection(inputs)
                                .optimize(self)
                                .filter(yanked_preds)
                                .optimize(self);
                        }
                    }
                }

                Expr::Intersection(inputs)
            }
            Expr::Filter { input, mut preds } => {
                // Eliminate trivial filter
                {
                    if preds.is_empty() {
                        return *input;
                    }
                }

                // Collapse multiple filters.
                {
                    match *input {
                        Expr::Filter {
                            input,
                            preds: inner_preds,
                        } => {
                            preds.extend(inner_preds.into_iter());
                            return Expr::Filter { input, preds }.optimize(self);
                        }
                        _ => {}
                    }
                }
                Expr::Filter {
                    input: Box::new(self.optimize(*input)),
                    preds,
                }
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
            Expr::Filter { input, preds } => {
                input.write_chain(out, indent)?;
                let method_pad = "  ".repeat(indent + 1);
                writeln!(out, "{}.filter({:?})", method_pad, preds)
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

    pub fn filter(self, preds: Vec<Predicate>) -> Self {
        Expr::Filter {
            input: Box::new(self),
            preds,
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
mod lang_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrites_ancestry() {
        let planner = Planner::default();
        let expr = Expr::constant(vec![Vx(0)])
            .up(0, None)
            .intersection(Expr::constant(vec![Vx(1)]).down(0, None))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"
        range(
          constant([Vx(0)]),
          constant([Vx(1)]),
        )
        ");
    }

    #[test]
    fn test_rewrites_filter() {
        let planner = Planner::default();
        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::All.filter(vec![Predicate::Author("Justin".into())]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @r#"
        constant([Vx(0)])
          .filter([Author("Justin")])
        "#);

        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::All.filter(vec![Predicate::Author("Justin".into())]))
            .intersection(Expr::All.filter(vec![Predicate::Description("Some description".into())]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @r#"
        constant([Vx(0)])
          .filter([Author("Justin"), Description("Some description")])
        "#);

        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::constant(vec![Vx(1)]).up(0, None))
            .intersection(Expr::All.filter(vec![Predicate::Description("Some description".into())]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @r#"
        intersection(
          constant([Vx(0)]),
          constant([Vx(1)])
            .up(0, *),
        )
          .filter([Description("Some description")])
        "#);

        let expr = Expr::constant(vec![Vx(0)])
            .filter(vec![Predicate::Author("Justin".into())])
            .union(Expr::constant(vec![Vx(1)]).filter(vec![Predicate::Author("Justin".into())]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @r#"
        constant([Vx(0), Vx(1)])
          .filter([Author("Justin")])
        "#);
    }

    #[test]
    fn test_rewrites() {
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

        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::constant(vec![Vx(1)]))
            .optimize(&planner);
        insta::assert_snapshot!(expr.tree().to_string(), @"none()");

        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::constant(vec![Vx(0)]))
            .optimize(&planner);
        insta::assert_snapshot!(expr.tree().to_string(), @"constant([Vx(0)])");

        let expr = Expr::constant(vec![Vx(0)])
            .intersection(Expr::All)
            .optimize(&planner);
        insta::assert_snapshot!(expr.tree().to_string(), @"constant([Vx(0)])");

        let expr = Expr::constant(vec![Vx(1)])
            .intersection(Expr::constant(vec![Vx(0)]))
            .optimize(&planner);
        insta::assert_snapshot!(expr.tree().to_string(), @"none()");

        let planner = Planner::default();
        let expr = Expr::constant(vec![])
            .intersection(Expr::constant(vec![]))
            .optimize(&planner);

        insta::assert_snapshot!(expr.tree().to_string(), @"none()");
    }
}
