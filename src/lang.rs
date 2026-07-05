use std::collections::HashMap;
use std::error::Error;

use jj_lib::fileset::FilesetAliasesMap;
use jj_lib::revset::{RevsetAliasesMap, RevsetDiagnostics, RevsetExtensions, RevsetParseContext};

pub(crate) fn parse_and_lower_to_planner(
    input: &str,
) -> Result<crate::planner::Expr, Box<dyn Error + Send + Sync + 'static>> {
    let aliases_map = RevsetAliasesMap::new();
    let fileset_aliases_map = FilesetAliasesMap::new();
    let extensions = RevsetExtensions::default();
    let context = RevsetParseContext {
        aliases_map: &aliases_map,
        local_variables: HashMap::new(),
        user_email: "",
        date_pattern_context: chrono::Utc::now().fixed_offset().into(),
        default_ignored_remote: None,
        fileset_aliases_map: &fileset_aliases_map,
        extensions: &extensions,
        workspace: None,
    };
    let parsed = jj_lib::revset::parse(&mut RevsetDiagnostics::new(), input, &context)?;
    Ok(crate::planner::lower_jj_revset(&parsed)?)
}
