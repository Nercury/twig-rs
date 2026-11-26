//! Port of TWIG/tests/BlockChainTest.php ([`BlockChain`]).
//!
//! The tests with the `yieldModes` data provider run with `use_yield` off and on, as upstream (both modes render
//! identically in twig-rs).
//!
//! Not ported:
//! - `testRenderCapturesLegacyEchoingBlocks` and `testRejectsInvalidBlockDefinitions`: hand-written subclasses of
//!   `Twig\Template` (a block method that echoes, a block table naming an object that is no template); twig-rs
//!   templates are always compiled.
//! - `testRejectsWrappersThatHideATemplateFromAnotherEnvironment`: builds a `TemplateWrapper` with its internal
//!   constructor around a template of another environment; twig-rs wrappers come from `Environment::load()` only.
//! - `testProfilerKeepsTheDefiningTemplateAndBlockAttribution`: twig-rs has no profiler.
//! - From `testRenderRestoresOutputBuffersOnError`, the PHP output buffer levels (twig-rs renders into Rust buffers);
//!   the failing render is ported.
//! - From `testChainsWithDifferentDynamicParentsCanBeStreamedInterleaved`, the interleaving of two PHP generators: a
//!   twig-rs stream writes to a sink until the block is done, so the two chains are streamed one after the other.

use std::sync::Arc;

use twig::escaper::DefaultStrategy;
use twig::runtime::{
    BlockChain, BlockChainTemplate, Environment, EnvironmentOptions, SandboxExtension,
    SecurityPolicy,
};
use twig::value::object::StdClass;
use twig::value::{Array, Value};
use twig::{Error, ErrorKind};

use crate::support::*;

/// `new Environment(new ArrayLoader($templates), ['autoescape' => false, 'use_yield' => $useYield])`.
fn twig(templates: &[(&str, &str)], use_yield: bool) -> Environment {
    env_with(
        templates,
        EnvironmentOptions {
            autoescape: DefaultStrategy::Disabled,
            use_yield,
            ..Default::default()
        },
    )
}

/// `yieldModes()`.
const YIELD_MODES: [bool; 2] = [false, true];

fn chain<T: Into<BlockChainTemplate>>(
    twig: &Environment,
    templates: impl IntoIterator<Item = T>,
) -> BlockChain {
    BlockChain::new(twig, templates, Array::new()).unwrap()
}

fn chain_with<T: Into<BlockChainTemplate>>(
    twig: &Environment,
    templates: impl IntoIterator<Item = T>,
    context: &[(&str, Value)],
) -> BlockChain {
    BlockChain::new(twig, templates, ctx(context)).unwrap()
}

fn s(value: &str) -> Value {
    Value::from(value)
}

fn parent(name: &str) -> Array {
    ctx(&[("parent", s(name))])
}

/// `new SecurityPolicy($tags, $filters, [], [], $functions)` (Twig 3's non-strict rules).
fn policy(tags: &[&str], filters: &[&str], functions: &[&str]) -> Arc<SecurityPolicy> {
    let policy = SecurityPolicy::new();
    policy.set_allowed_tags(tags.iter().copied());
    policy.set_allowed_filters(filters.iter().copied());
    policy.set_allowed_functions(functions.iter().copied());
    Arc::new(policy)
}

/// `$twig->addExtension($sandbox = new SandboxExtension($policy, true))`.
fn add_sandbox(twig: &Environment, policy: Arc<SecurityPolicy>) -> Arc<SandboxExtension> {
    let sandbox = Arc::new(SandboxExtension::new(policy, true));
    twig.add_extension(Arc::clone(&sandbox) as _).unwrap();
    sandbox
}

#[track_caller]
fn expect(result: twig::Result<String>, kind: ErrorKind) -> Error {
    expect_error(result, kind)
}

#[test]
fn test_composes_complete_template_lineages_in_precedence_order() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme1",
                    "{% extends \"parent1\" %}{% block first %}theme1{% endblock %}",
                ),
                (
                    "parent1",
                    "{% block shared %}parent1{% endblock %}{% block parent1 %}parent1{% endblock %}",
                ),
                (
                    "theme2",
                    "{% extends \"parent2\" %}{% block shared %}theme2{% endblock %}{% block second %}theme2{% endblock %}",
                ),
                ("parent2", "{% block parent2 %}parent2{% endblock %}"),
            ],
            use_yield,
        );
        let chain = chain(
            &twig,
            [
                BlockChainTemplate::from("theme1"),
                BlockChainTemplate::from(twig.load("theme2").unwrap()),
            ],
        );
        assert_eq!(
            chain.block_names(Array::new()).unwrap(),
            ["first", "shared", "parent1", "second", "parent2"]
        );
        assert!(chain.has_block("shared", Array::new()).unwrap());
        assert!(!chain.has_block("missing", Array::new()).unwrap());
        assert_eq!(
            chain.render_block("shared", Array::new()).unwrap(),
            "parent1"
        );
        assert_eq!(
            chain.render_block("second", Array::new()).unwrap(),
            "theme2"
        );
    }
}

#[test]
fn test_nested_blocks_use_the_composed_set_and_parent_uses_the_render_context_lineage() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends parent %}{% block field %}theme/{{ parent() }}/{{ block(\"suffix\") }}{% endblock %}",
                ),
                ("parent1", "{% block field %}parent1{% endblock %}"),
                ("parent2", "{% block field %}parent2{% endblock %}"),
                ("suffix", "{% block suffix %}suffix{% endblock %}"),
            ],
            use_yield,
        );
        let chain = chain_with(&twig, ["theme", "suffix"], &[("parent", s("parent1"))]);
        assert_eq!(
            chain.render_block("field", Array::new()).unwrap(),
            "theme/parent1/suffix"
        );
        assert_eq!(
            chain.render_block("field", parent("parent2")).unwrap(),
            "theme/parent2/suffix"
        );
    }
}

#[test]
fn test_explicit_template_block_calls_resolve_outside_the_chain_namespace() {
    let twig = twig(
        &[
            (
                "theme",
                "{% block field %}{{ block(\"suffix\", \"explicit\") }}{% endblock %}",
            ),
            ("chain", "{% block suffix %}chain{% endblock %}"),
            ("explicit", "{% block suffix %}explicit{% endblock %}"),
        ],
        true,
    );
    let chain = chain(&twig, ["theme", "chain"]);
    assert_eq!(
        chain.render_block("field", Array::new()).unwrap(),
        "explicit"
    );
}

#[test]
fn test_the_constructor_context_is_a_default_the_render_context_can_override() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends parent %}{% block field %}{{ parent() }}{% endblock %}",
            ),
            ("parent1", "{% block field %}one{% endblock %}"),
            ("parent2", "{% block field %}two{% endblock %}"),
        ],
        true,
    );
    let chain = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
    assert_eq!(
        chain.render_block("field", parent("parent2")).unwrap(),
        "two"
    );
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
}

#[test]
fn test_introspection_and_rendering_agree_on_the_same_context() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends parent %}{% block field %}{{ parent() }}{% endblock %}",
            ),
            (
                "parent1",
                "{% block field %}one{% endblock %}{% block only1 %}{% endblock %}",
            ),
            (
                "parent2",
                "{% block field %}two{% endblock %}{% block only2 %}{% endblock %}",
            ),
        ],
        true,
    );
    let chain = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
    assert_eq!(chain.block_names(Array::new()).unwrap(), ["field", "only1"]);
    assert!(chain.has_block("only1", Array::new()).unwrap());
    assert!(!chain.has_block("only2", Array::new()).unwrap());

    let context = parent("parent2");
    assert_eq!(
        chain.block_names(context.clone()).unwrap(),
        ["field", "only2"]
    );
    assert!(!chain.has_block("only1", context.clone()).unwrap());
    assert!(chain.has_block("only2", context.clone()).unwrap());
    assert_eq!(chain.render_block("field", context).unwrap(), "two");
}

#[test]
fn test_rendering_through_the_chain_does_not_change_the_loaded_template() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends parent %}{% block field %}{{ parent() }}{% endblock %}",
            ),
            ("parent1", "{% block field %}one{% endblock %}"),
            ("parent2", "{% block field %}two{% endblock %}"),
        ],
        true,
    );
    let template = twig.load("theme").unwrap();
    let chain = chain_with(&twig, [&template], &[("parent", s("parent1"))]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
    assert_eq!(
        template.render_block("field", parent("parent2")).unwrap(),
        "two"
    );
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
}

#[test]
fn test_structural_context_includes_environment_globals() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends layout %}{% block field %}{{ parent() }}{% endblock %}",
            ),
            ("parent", "{% block field %}parent{% endblock %}"),
        ],
        true,
    );
    twig.add_global("layout", "parent").unwrap();
    let chain = chain(&twig, ["theme"]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "parent");
}

#[test]
fn test_trait_aliases_keep_their_local_parent_lineage() {
    let twig = twig(
        &[
            ("base_trait", "{% block field %}base{% endblock %}"),
            (
                "trait",
                "{% use \"base_trait\" %}{% block field %}trait/{{ parent() }}{% endblock %}",
            ),
            ("theme", "{% use \"trait\" with field as aliased %}"),
        ],
        true,
    );
    let chain = chain(&twig, ["theme"]);
    assert_eq!(chain.block_names(Array::new()).unwrap(), ["aliased"]);
    assert_eq!(
        chain.render_block("aliased", Array::new()).unwrap(),
        "trait/base"
    );
}

/// `testRenderRestoresOutputBuffersOnError()`, without the PHP output buffers: the render fails, and the chain
/// renders again afterwards.
#[test]
fn test_render_restores_output_buffers_on_error() {
    let twig = env_with(
        &[(
            "theme",
            "{% block field %}{% set captured %}{{ missing.value }}{% endset %}{% endblock %}",
        )],
        EnvironmentOptions {
            strict_variables: true,
            use_yield: false,
            ..Default::default()
        },
    );
    let chain = chain(&twig, ["theme"]);
    let error = expect(
        chain.render_block("field", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Variable \"missing\" does not exist in \"theme\" at line 1."
    );
    let error = expect(
        chain.render_block("field", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Variable \"missing\" does not exist in \"theme\" at line 1."
    );
}

#[test]
fn test_rendering_displaying_and_streaming_add_globals() {
    let twig = twig(
        &[(
            "theme",
            "{% block field %}{{ local }}:{{ global|default(\"none\") }}{% endblock %}",
        )],
        true,
    );
    twig.add_global("global", "GLOBAL").unwrap();
    let chain = chain(&twig, ["theme"]);
    let local = || ctx(&[("local", s("LOCAL"))]);

    assert_eq!(
        chain.render_block("field", local()).unwrap(),
        "LOCAL:GLOBAL"
    );
    let mut displayed: Vec<u8> = Vec::new();
    chain
        .display_block("field", local(), &mut displayed)
        .unwrap();
    assert_eq!(displayed, b"LOCAL:GLOBAL");
    let mut streamed: Vec<u8> = Vec::new();
    chain.stream_block("field", local(), &mut streamed).unwrap();
    assert_eq!(streamed, b"LOCAL:GLOBAL");
}

#[test]
fn test_rejects_templates_that_are_not_strings_wrappers_or_chains() {
    let twig = twig(&[("theme", "")], false);
    let error = BlockChain::new(
        &twig,
        [Value::Object(StdClass::new().into_ref())],
        Array::new(),
    )
    .unwrap_err();
    assert_eq!(error.class_name(), "TypeError");
    assert_eq!(
        error.to_string(),
        "Block chain templates must be strings, \"Twig\\TemplateWrapper\" or \"Twig\\BlockChain\" instances, \"stdClass\" given."
    );
}

#[test]
fn test_a_chained_chain_keeps_its_position_in_the_precedence_order() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                ("override", "{% block field %}override{% endblock %}"),
                (
                    "base",
                    "{% block field %}base{% endblock %}{% block shared %}base{% endblock %}",
                ),
                (
                    "fallback",
                    "{% block shared %}fallback{% endblock %}{% block last %}fallback{% endblock %}",
                ),
            ],
            use_yield,
        );
        let base = chain(&twig, ["base"]);
        let chain = chain(
            &twig,
            [
                BlockChainTemplate::from("override"),
                BlockChainTemplate::from(&base),
                BlockChainTemplate::from("fallback"),
            ],
        );
        assert_eq!(
            chain.block_names(Array::new()).unwrap(),
            ["field", "shared", "last"]
        );
        assert_eq!(
            chain.render_block("field", Array::new()).unwrap(),
            "override"
        );
        assert_eq!(chain.render_block("shared", Array::new()).unwrap(), "base");
        assert_eq!(
            chain.render_block("last", Array::new()).unwrap(),
            "fallback"
        );
        // the chained chain is left untouched
        assert_eq!(base.block_names(Array::new()).unwrap(), ["field", "shared"]);
        assert_eq!(base.render_block("field", Array::new()).unwrap(), "base");
    }
}

#[test]
fn test_nested_block_calls_see_the_blocks_of_a_chained_chain() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "override",
                    "{% block field %}override/{{ block(\"widget\") }}{% endblock %}",
                ),
                (
                    "base",
                    "{% block field %}base{% endblock %}{% block widget %}base-widget{% endblock %}",
                ),
            ],
            use_yield,
        );
        let chain = chain(
            &twig,
            [
                BlockChainTemplate::from("override"),
                BlockChainTemplate::from(chain(&twig, ["base"])),
            ],
        );
        assert_eq!(
            chain.render_block("field", Array::new()).unwrap(),
            "override/base-widget"
        );
    }
}

#[test]
fn test_a_chained_chain_is_composed_through_its_whole_lineage() {
    let twig = twig(
        &[
            ("override", "{% block field %}override{% endblock %}"),
            (
                "base",
                "{% extends \"layout\" %}{% block widget %}base-widget{% endblock %}",
            ),
            (
                "layout",
                "{% block field %}layout{% endblock %}{% block row %}layout-row{% endblock %}",
            ),
        ],
        true,
    );
    let chain = chain(
        &twig,
        [
            BlockChainTemplate::from("override"),
            BlockChainTemplate::from(chain(&twig, ["base"])),
        ],
    );
    assert_eq!(
        chain.block_names(Array::new()).unwrap(),
        ["field", "widget", "row"]
    );
    assert_eq!(
        chain.render_block("field", Array::new()).unwrap(),
        "override"
    );
    assert_eq!(
        chain.render_block("row", Array::new()).unwrap(),
        "layout-row"
    );
}

#[test]
fn test_a_chained_chain_keeps_its_own_default_context() {
    let twig = twig(
        &[
            ("theme", "{% extends parent %}"),
            ("parent1", "{% block field %}one{% endblock %}"),
            ("parent2", "{% block field %}two{% endblock %}"),
        ],
        true,
    );
    let base = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
    let chain = chain(&twig, [base]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
    assert_eq!(
        chain.render_block("field", parent("parent2")).unwrap(),
        "two"
    );
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
}

#[test]
fn test_a_chained_chain_with_a_dynamic_parent_is_resolved_against_the_render_context() {
    let twig = twig(
        &[
            ("theme", "{% extends parent %}"),
            (
                "parent1",
                "{% block field %}one{% endblock %}{% block only1 %}{% endblock %}",
            ),
            (
                "parent2",
                "{% block field %}two{% endblock %}{% block only2 %}{% endblock %}",
            ),
        ],
        true,
    );
    let chain = chain(&twig, [chain(&twig, ["theme"])]);
    let context = parent("parent1");
    assert_eq!(
        chain.block_names(context.clone()).unwrap(),
        ["field", "only1"]
    );
    assert_eq!(chain.render_block("field", context).unwrap(), "one");
    let context = parent("parent2");
    assert_eq!(
        chain.block_names(context.clone()).unwrap(),
        ["field", "only2"]
    );
    assert_eq!(chain.render_block("field", context).unwrap(), "two");
}

#[test]
fn test_rejects_chains_from_another_environment() {
    let twig = twig(&[("theme", "")], false);
    let other = env(&[("theme", "")]);
    let error = BlockChain::new(&twig, [chain(&other, ["theme"])], Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "A block chain cannot contain a block chain from a different Twig environment."
    );
}

#[test]
fn test_rejects_wrappers_from_another_environment() {
    let twig = twig(&[("theme", "")], false);
    let other = env(&[("theme", "")]);
    let error = BlockChain::new(&twig, [other.load("theme").unwrap()], Array::new()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "A block chain cannot contain templates from different Twig environments."
    );
}

#[test]
fn test_rejects_dynamic_parents_from_another_environment() {
    let twig = env(&[("theme", "{% extends parent %}")]);
    let other = env(&[("parent", "")]);
    let chain = chain_with(
        &twig,
        ["theme"],
        &[("parent", other.load("parent").unwrap().to_value())],
    );
    let error = expect_error(chain.block_names(Array::new()), ErrorKind::Runtime);
    assert_eq!(
        error.to_string(),
        "A \"Twig\\TemplateWrapper\" can only be used with the \"Twig\\Environment\" that created it in \"theme\" at line 1."
    );
}

#[test]
fn test_dynamic_parent_security_is_checked_when_the_lineage_is_resolved() {
    let twig = env(&[("theme", "{% extends parent|upper %}"), ("PARENT", "")]);
    add_sandbox(&twig, policy(&["extends"], &[], &[]));
    let chain = chain_with(&twig, ["theme"], &[("parent", s("parent"))]);
    expect_error(
        chain.block_names(Array::new()),
        ErrorKind::SecurityNotAllowedFilter,
    );
}

#[test]
fn test_defining_template_security_is_checked_during_rendering() {
    let twig = env(&[("theme", "{% block field %}{{ value|upper }}{% endblock %}")]);
    add_sandbox(&twig, policy(&["block"], &[], &[]));
    let chain = chain(&twig, ["theme"]);
    expect(
        chain.render_block("field", ctx(&[("value", s("value"))])),
        ErrorKind::SecurityNotAllowedFilter,
    );
}

#[test]
fn test_sandbox_policy_changes_are_observed_after_construction() {
    let twig = twig(
        &[
            (
                "policy_theme",
                "{% extends \"parent\" %}{% block field %}{{ value|upper }}{% endblock %}",
            ),
            ("parent", ""),
        ],
        true,
    );
    let sandbox = add_sandbox(&twig, policy(&["extends", "block"], &["upper"], &[]));
    let chain = chain(&twig, ["policy_theme"]);
    let value = || ctx(&[("value", s("value"))]);
    assert_eq!(chain.render_block("field", value()).unwrap(), "VALUE");

    sandbox.set_security_policy(policy(&["extends", "block"], &[], &[]));
    expect(
        chain.render_block("field", value()),
        ErrorKind::SecurityNotAllowedFilter,
    );
}

#[test]
fn test_sandbox_policy_changes_are_checked_on_intermediate_parents() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"middle\" %}{% block field %}{{ parent() }}{% endblock %}",
                ),
                ("middle", "{% extends parent|upper %}"),
                ("GRANDPARENT", "{% block field %}safe{% endblock %}"),
            ],
            use_yield,
        );
        let sandbox = add_sandbox(
            &twig,
            policy(&["extends", "block"], &["upper"], &["parent"]),
        );
        let chain = chain_with(&twig, ["theme"], &[("parent", s("grandparent"))]);
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "safe");

        sandbox.set_security_policy(policy(&["extends", "block"], &[], &["parent"]));
        expect(
            chain.render_block("field", Array::new()),
            ErrorKind::SecurityNotAllowedFilter,
        );
    }
}

#[test]
fn test_imported_macro_namespaces_observe_sandbox_policy_changes() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"layout\" %}{% import \"macros\" as macros %}{% block field %}{{ macros.label(value) }}{% endblock %}",
                ),
                ("layout", "{{ block(\"field\") }}"),
                (
                    "macros",
                    "{% macro label(value) %}{{ value|upper }}{% endmacro %}",
                ),
            ],
            use_yield,
        );
        let sandbox = add_sandbox(
            &twig,
            policy(
                &["extends", "import", "block", "macro"],
                &["upper"],
                &["block"],
            ),
        );
        let chain = chain(&twig, ["theme"]);
        let value = || ctx(&[("value", s("value"))]);
        assert_eq!(twig.render("theme", value()).unwrap(), "VALUE");
        assert_eq!(chain.render_block("field", value()).unwrap(), "VALUE");

        sandbox.set_security_policy(policy(
            &["extends", "import", "block", "macro"],
            &[],
            &["block"],
        ));
        expect(
            chain.render_block("field", value()),
            ErrorKind::SecurityNotAllowedFilter,
        );
    }
}

#[test]
fn test_macros_stay_owned_by_their_defining_template() {
    let twig = twig(
        &[
            (
                "theme1",
                "{% macro label() %}one{% endmacro %}{% block field %}{{ _self.label() }}{% endblock %}",
            ),
            (
                "theme2",
                "{% macro label() %}two{% endmacro %}{% block field %}{{ _self.label() }}{% endblock %}",
            ),
        ],
        true,
    );
    let chain = chain(&twig, ["theme1", "theme2"]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
}

#[test]
fn test_inherited_macro_lookup_uses_the_render_context_lineage() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends parent %}{% block field %}{{ _self.label() }}{% endblock %}",
            ),
            ("parent1", "{% macro label() %}one{% endmacro %}"),
            ("parent2", "{% macro label() %}two{% endmacro %}"),
        ],
        true,
    );
    let chain = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
    assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
    assert_eq!(
        chain.render_block("field", parent("parent2")).unwrap(),
        "two"
    );
}

#[test]
fn test_self_macro_imports_use_the_render_context_lineage() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends parent %}{% import _self as own %}{% block field %}{{ own.label() }}{% endblock %}",
                ),
                (
                    "parent1",
                    "{% macro label() %}one{% endmacro %}{{ block(\"field\") }}",
                ),
                (
                    "parent2",
                    "{% macro label() %}two{% endmacro %}{{ block(\"field\") }}",
                ),
            ],
            use_yield,
        );
        let chain = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
        assert_eq!(twig.render("theme", parent("parent2")).unwrap(), "two");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
        assert_eq!(
            chain.render_block("field", parent("parent2")).unwrap(),
            "two"
        );
    }
}

#[test]
fn test_from_self_imports_resolve_through_the_lineage() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"parent\" %}{% from _self import label %}{% macro wrapped() %}{{ label() }}{% endmacro %}{% block field %}{{ label() }}/{{ _self.wrapped() }}{% endblock %}",
                ),
                ("parent", "{% macro label() %}one{% endmacro %}"),
            ],
            use_yield,
        );
        assert_eq!(twig.render("theme", Array::new()).unwrap(), "");
        let chain = chain(&twig, ["theme"]);
        assert_eq!(
            chain.render_block("field", Array::new()).unwrap(),
            "one/one"
        );
    }
}

#[test]
fn test_imported_macro_namespaces_keep_their_own_lineage() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends parent %}{% import parent as inherited %}{% block field %}{{ inherited.label() }}{% endblock %}",
                ),
                (
                    "parent1",
                    "{% macro label() %}one{% endmacro %}{{ block(\"field\") }}",
                ),
                (
                    "parent2",
                    "{% macro label() %}two{% endmacro %}{{ block(\"field\") }}",
                ),
            ],
            use_yield,
        );
        assert_eq!(twig.render("theme", parent("parent2")).unwrap(), "two");
        let chain = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "two");
    }
}

#[test]
fn test_module_imports_follow_the_defining_template_body_state() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"layout\" %}{% import helper as macros %}{% block field %}{{ macros.label() }}{% endblock %}",
                ),
                ("layout", "{{ block(\"field\") }}"),
                ("macros1", "{% macro label() %}one{% endmacro %}"),
                ("macros2", "{% macro label() %}two{% endmacro %}"),
            ],
            use_yield,
        );
        let chain = chain(&twig, ["theme"]);
        let helper = |name: &str| ctx(&[("helper", s(name))]);
        let error = expect(
            chain.render_block("field", helper("macros1")),
            ErrorKind::Runtime,
        );
        assert!(
            error.to_string().contains(
                "Macros imported in the body of template \"theme\" are not available because the body was not rendered"
            ),
            "{error}"
        );
        assert_eq!(twig.render("theme", helper("macros1")).unwrap(), "one");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
        assert_eq!(twig.render("theme", helper("macros2")).unwrap(), "two");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "two");
    }
}

#[test]
fn test_self_macro_imports_in_macro_bodies_resolve_through_the_lineage() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"parent\" %}{% import _self as own %}{% macro wrapped() %}{{ own.label() }}{% endmacro %}{% block field %}{{ _self.wrapped() }}{% endblock %}",
                ),
                ("parent", "{% macro label() %}one{% endmacro %}"),
            ],
            use_yield,
        );
        let chain = chain(&twig, ["theme"]);
        assert_eq!(twig.render("theme", Array::new()).unwrap(), "");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
    }
}

#[test]
fn test_macro_bodies_cannot_resolve_a_dynamic_parent_just_like_a_direct_render() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends parent %}{% import _self as own %}{% macro wrapped() %}{{ own.label() }}{% endmacro %}{% block field %}{{ _self.wrapped() }}{% endblock %}",
                ),
                (
                    "parent",
                    "{% macro label() %}one{% endmacro %}{{ block(\"field\") }}",
                ),
            ],
            use_yield,
        );
        let chain = chain_with(&twig, ["theme"], &[("parent", s("parent"))]);
        // a macro body only receives the macro arguments, so the "parent" variable is out of reach either way
        expect(twig.render("theme", parent("parent")), ErrorKind::Runtime);
        expect(
            chain.render_block("field", Array::new()),
            ErrorKind::Runtime,
        );
    }
}

#[test]
fn test_macro_bodies_observe_import_updates_after_construction() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"layout\" %}{% import helper as macros %}{% macro wrapped() %}{{ macros.label() }}{% endmacro %}{% block field %}{{ _self.wrapped() }}{% endblock %}",
                ),
                ("layout", "{{ block(\"field\") }}"),
                ("macros1", "{% macro label() %}one{% endmacro %}"),
                ("macros2", "{% macro label() %}two{% endmacro %}"),
            ],
            use_yield,
        );
        let chain = chain(&twig, ["theme"]);
        let helper = |name: &str| ctx(&[("helper", s(name))]);
        assert_eq!(twig.render("theme", helper("macros1")).unwrap(), "one");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "one");
        assert_eq!(twig.render("theme", helper("macros2")).unwrap(), "two");
        assert_eq!(chain.render_block("field", Array::new()).unwrap(), "two");
    }
}

#[test]
fn test_pre_warmed_external_macro_imports_are_not_rebound_by_chain_order() {
    for use_yield in YIELD_MODES {
        let twig = twig(
            &[
                (
                    "theme",
                    "{% extends \"layout\" %}{% import \"macros\" as macros %}{% block field %}{{ macros.label() }}{% endblock %}",
                ),
                ("layout", "{{ block(\"field\") }}"),
                ("macros", "{% extends macro_parent %}"),
                ("macros1", "{% macro label() %}one{% endmacro %}"),
                ("macros2", "{% macro label() %}two{% endmacro %}"),
            ],
            use_yield,
        );
        let macro_parent = |name: &str| ctx(&[("macro_parent", s(name))]);
        assert_eq!(
            twig.render("theme", macro_parent("macros2")).unwrap(),
            "two"
        );
        let context = [("macro_parent", s("macros1"))];
        assert_eq!(
            chain_with(&twig, ["macros", "theme"], &context)
                .render_block("field", macro_parent("macros2"))
                .unwrap(),
            "two"
        );
        assert_eq!(
            chain_with(&twig, ["theme", "macros"], &context)
                .render_block("field", macro_parent("macros2"))
                .unwrap(),
            "two"
        );
    }
}

#[test]
fn test_chains_with_different_dynamic_parents_can_be_streamed_interleaved() {
    let twig = twig(
        &[
            (
                "theme",
                "{% extends parent %}{% block field %}before/{{ parent() }}/after{% endblock %}",
            ),
            ("parent1", "{% block field %}one{% endblock %}"),
            ("parent2", "{% block field %}two{% endblock %}"),
        ],
        true,
    );
    let chain1 = chain_with(&twig, ["theme"], &[("parent", s("parent1"))]);
    let chain2 = chain_with(&twig, ["theme"], &[("parent", s("parent2"))]);
    let mut output1: Vec<u8> = Vec::new();
    let mut output2: Vec<u8> = Vec::new();
    chain1
        .stream_block("field", Array::new(), &mut output1)
        .unwrap();
    chain2
        .stream_block("field", Array::new(), &mut output2)
        .unwrap();
    assert_eq!(output1, b"before/one/after");
    assert_eq!(output2, b"before/two/after");
}

#[test]
fn test_errors_keep_the_defining_source_and_line() {
    let twig = env_with(
        &[(
            "theme",
            "{% block field %}\n{{ missing.value }}\n{% endblock %}",
        )],
        EnvironmentOptions {
            strict_variables: true,
            use_yield: true,
            ..Default::default()
        },
    );
    let chain = chain(&twig, ["theme"]);
    let error = expect(
        chain.render_block("field", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(error.source_context().map(|s| s.name()), Some("theme"));
    assert_eq!(error.template_line(), 2);
}

#[test]
fn test_unknown_block_uses_the_first_template_as_error_context() {
    let twig = env(&[("theme", "")]);
    let chain = chain(&twig, ["theme"]);
    let error = expect(
        chain.render_block("missing", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Block \"missing\" on template \"theme\" does not exist in \"theme\"."
    );
}

#[test]
fn test_unknown_block_uses_the_first_template_of_a_chained_chain_as_error_context() {
    let twig = env(&[("theme", "")]);
    let chain = chain(&twig, [chain(&twig, ["theme"])]);
    let error = expect(
        chain.render_block("missing", Array::new()),
        ErrorKind::Runtime,
    );
    assert_eq!(
        error.to_string(),
        "Block \"missing\" on template \"theme\" does not exist in \"theme\"."
    );
}

#[test]
fn test_circular_inheritance_is_rejected() {
    let twig = env(&[
        ("one", "{% extends \"two\" %}"),
        ("two", "{% extends \"one\" %}"),
    ]);
    let chain = chain(&twig, ["one"]);
    let error = expect_error(chain.block_names(Array::new()), ErrorKind::Logic);
    assert_eq!(
        error.to_string(),
        "Circular template inheritance detected while building a block chain from \"one\"."
    );
}

#[test]
fn test_requires_at_least_one_template() {
    let error = BlockChain::new(&env(&[]), Vec::<&str>::new(), Array::new()).unwrap_err();
    assert_eq!(error.class_name(), "InvalidArgumentException");
    assert_eq!(
        error.to_string(),
        "A block chain requires at least one template."
    );
}
