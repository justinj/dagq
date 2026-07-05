use super::*;
use crate::lang;

fn lower(query: &str) -> Expr {
    lang::parse_and_lower_to_planner(query).unwrap()
}

fn optimized(query: &str) -> Expr {
    lower(query).optimize(&Planner::default())
}

macro_rules! optimized_tree {
    ($input:expr $(,)?) => {{ optimized($input).tree().to_string() }};
    ($planner:expr, $input:expr $(,)?) => {{ lower($input).optimize($planner).tree().to_string() }};
}

#[test]
fn rewrites_ancestry_from_language() {
    insta::assert_snapshot!(optimized_tree!("0:: & ::1"), @"
    range(
      constant([Vx(0)]),
      constant([Vx(1)]),
    )
    ");
}

#[test]
fn rewrites_filter_from_language() {
    insta::assert_snapshot!(optimized_tree!("0 & author(Justin)"), @r#"
    constant([Vx(0)])
      .filter([Author("Justin")])
    "#);

    insta::assert_snapshot!(optimized_tree!("0 & author(Justin) & author(Hamlet)"), @r#"
    constant([Vx(0)])
      .filter([Author("Justin"), Author("Hamlet")])
    "#);

    insta::assert_snapshot!(optimized_tree!("0 & author(Justin) & description(Some-description)"), @r#"
    constant([Vx(0)])
      .filter([Author("Justin"), Description("Some-description")])
    "#);

    insta::assert_snapshot!(optimized_tree!("0 & 1:: & description(Some-description)"), @r#"
    intersection(
      constant([Vx(0)]),
      constant([Vx(1)])
        .up(0, *),
    )
      .filter([Description("Some-description")])
    "#);

    insta::assert_snapshot!(optimized_tree!("(0 & author(Justin)) | (1 & author(Justin))"), @r#"
    constant([Vx(0), Vx(1)])
      .filter([Author("Justin")])
    "#);
}

#[test]
fn rewrites_up_from_language() {
    insta::assert_snapshot!(optimized_tree!("(0::)::"), @"
    constant([Vx(0)])
      .up(0, *)
    ");

    insta::assert_snapshot!(optimized_tree!(
        &Planner {
            rules: RewriteRules {
                fold_up_up: false,
                ..RewriteRules::all()
            },
        },
        "0::",
    ), @"
    constant([Vx(0)])
      .up(0, *)
    ");
}

#[test]
fn rewrites_constant_sets_from_language() {
    insta::assert_snapshot!(optimized_tree!("0 | 1"), @"constant([Vx(0), Vx(1)])");
    insta::assert_snapshot!(optimized_tree!("1 | 0"), @"constant([Vx(0), Vx(1)])");
    insta::assert_snapshot!(optimized_tree!("0 & 1"), @"none()");
    insta::assert_snapshot!(optimized_tree!("0 & 0"), @"constant([Vx(0)])");
    insta::assert_snapshot!(optimized_tree!("1 & 0"), @"none()");
}
