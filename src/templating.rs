//! MiniJinja template resolution.
//!
//! LSPD resolves templates; clients consume the effective, already
//! rendered configuration (spec §6, §18, ADR-0006). This module renders
//! every string leaf of an LSP definition's `command`/`args`/`environment`
//! and the string leaves nested inside its `initialization_options`/
//! `settings`/`capabilities` JSON blobs, against one context built once per
//! resolution.

use std::collections::HashMap;
use std::path::Path;

use minijinja::{Environment, UndefinedBehavior, context};

use crate::error::Error;

/// The render context exposed to every template in a project's configuration.
///
/// Deliberately a thin wrapper around a prebuilt [`minijinja::Value`]-ish
/// context rather than a long-lived `Environment` with registered templates:
/// every string in the configuration is rendered independently as an ad-hoc
/// template (`render_str`), since definitions are data, not named templates
/// reused across projects.
pub struct TemplateContext {
    env: HashMap<String, String>,
    project_root: String,
    project_name: String,
}

impl TemplateContext {
    /// Builds a context exposing `env.*` (the fully resolved environment —
    /// OS env merged with the Mise-provided environment, see `crate::mise`)
    /// and `project.root`/`project.name`. The spec asks that the context "be
    /// extensible for future project/runtime values" (§6) — `project.*` is
    /// where those land.
    #[must_use]
    pub fn new(env: HashMap<String, String>, root: &Path) -> Self {
        Self {
            env,
            project_root: root.display().to_string(),
            project_name: root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
        }
    }

    /// Renders a single template string, e.g. `"{{ env.PROJECT_ROOT }}/src"`.
    /// A string with no `{{`/`{%` in it renders to itself — most
    /// configuration values are plain strings, and short-circuiting them
    /// avoids paying MiniJinja's parse cost on every leaf of every
    /// definition.
    pub fn render(&self, template: &str) -> crate::Result<String> {
        if !template.contains("{{") && !template.contains("{%") {
            return Ok(template.to_string());
        }

        let mut env = Environment::new();
        // A misspelled `env.*` reference must fail loudly, not silently
        // render as an empty string that corrupts a path — matching the
        // "undeclared is an error" discipline this project's own template
        // stack follows for the same reason.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        let ctx = context! {
            env => self.env,
            project => context! {
                root => self.project_root,
                name => self.project_name,
            },
        };
        env.render_str(template, ctx)
            .map_err(|source| Error::TemplateRender {
                template: template.to_string(),
                source: Box::new(source),
            })
    }

    /// Renders every string leaf of a JSON value in place, recursing through
    /// objects and arrays. Numbers/bools/null pass through untouched — only
    /// strings can contain `{{ }}` templates.
    pub fn render_json(&self, value: serde_json::Value) -> crate::Result<serde_json::Value> {
        use serde_json::Value;
        Ok(match value {
            Value::String(s) => Value::String(self.render(&s)?),
            Value::Array(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| self.render_json(item))
                    .collect::<crate::Result<_>>()?,
            ),
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| Ok((k, self.render_json(v)?)))
                    .collect::<crate::Result<_>>()?,
            ),
            other => other,
        })
    }

    /// Renders every value in a `String -> String` map (used for
    /// `environment`), keeping keys untouched.
    pub fn render_map(
        &self,
        map: HashMap<String, String>,
    ) -> crate::Result<HashMap<String, String>> {
        map.into_iter()
            .map(|(k, v)| Ok((k, self.render(&v)?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ctx() -> TemplateContext {
        TemplateContext::new(
            HashMap::from([("PROJECT_ROOT".to_string(), "/work/demo".to_string())]),
            &PathBuf::from("/work/demo"),
        )
    }

    #[test]
    fn a_plain_string_with_no_template_syntax_renders_unchanged() {
        assert_eq!(
            ctx().render("pyright-langserver").unwrap(),
            "pyright-langserver"
        );
    }

    #[test]
    fn env_star_exposes_the_resolved_environment() {
        assert_eq!(
            ctx().render("{{ env.PROJECT_ROOT }}/src").unwrap(),
            "/work/demo/src"
        );
    }

    #[test]
    fn project_root_and_name_are_exposed_for_future_use() {
        assert_eq!(ctx().render("{{ project.name }}").unwrap(), "demo");
    }

    #[test]
    fn an_undefined_variable_is_a_render_error_not_an_empty_string() {
        // Without strict mode this would silently render `{{ env.TYPO }}/src`
        // as `/src`, turning a typo into a hard-to-notice wrong path.
        let err = ctx().render("{{ env.DOES_NOT_EXIST }}");
        assert!(err.is_err());
    }

    #[test]
    fn json_string_leaves_are_rendered_recursively() {
        let value = serde_json::json!({
            "python": {"analysis": {"extraPaths": ["{{ env.PROJECT_ROOT }}/src"]}}
        });
        let rendered = ctx().render_json(value).unwrap();
        assert_eq!(
            rendered,
            serde_json::json!({"python": {"analysis": {"extraPaths": ["/work/demo/src"]}}})
        );
    }
}
