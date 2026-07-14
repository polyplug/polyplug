//! Shared bridge from Polyplug language rules to LangPrint attribute rendering.
//!
//! Rules store only attribute bodies. LangPrint owns target-specific delimiters
//! and site syntax so generators never need to pre-wrap a rule value.

use crate::Lang;
use crate::ir::{CustomizableNode, LanguageRules};
use langprint::TargetLanguage;
use langprint::ir::{AttributeSite, RawAttribute, render_raw_attributes};

/// Map a Polyplug generator language to LangPrint's target language.
///
/// This match intentionally has no fallback: adding a generator language must
/// also select its LangPrint backend here.
pub(crate) const fn target_language(lang: Lang) -> TargetLanguage {
    match lang {
        Lang::Rust => TargetLanguage::Rust,
        Lang::Cpp => TargetLanguage::Cpp,
        Lang::CSharp => TargetLanguage::CSharp,
        Lang::Python => TargetLanguage::Python,
        Lang::Lua => TargetLanguage::Lua,
        Lang::JsQuickJs => TargetLanguage::Js,
    }
}

/// Map an authored API node to the LangPrint declaration site that owns it.
///
/// Guest and host contracts are type declarations in every generated backend.
/// This match intentionally has no fallback: a new customizable node must
/// choose its rendering site.
pub(crate) const fn attribute_site(node: CustomizableNode) -> AttributeSite {
    match node {
        CustomizableNode::Api => AttributeSite::Root,
        CustomizableNode::Type => AttributeSite::Type,
        CustomizableNode::Field => AttributeSite::Field,
        CustomizableNode::Enum => AttributeSite::Enum,
        CustomizableNode::EnumVariant => AttributeSite::Variant,
        CustomizableNode::GuestContract => AttributeSite::Type,
        CustomizableNode::HostContract => AttributeSite::Type,
        CustomizableNode::Function => AttributeSite::Function,
        CustomizableNode::Param => AttributeSite::Parameter,
        CustomizableNode::Return => AttributeSite::Return,
    }
}

/// Return the selected language's unwrapped attribute bodies in authored order.
pub(crate) fn inner_attributes(lang: Lang, rules: &LanguageRules) -> &[String] {
    rules
        .for_lang(lang)
        .map_or(&[], |attributes| attributes.attributes.as_slice())
}

/// Convert selected inner attribute bodies into LangPrint source-tagged values.
pub(crate) fn raw_attributes(lang: Lang, rules: &LanguageRules) -> Vec<RawAttribute> {
    let source: TargetLanguage = target_language(lang);
    inner_attributes(lang, rules)
        .iter()
        .cloned()
        .map(|text: String| RawAttribute { source, text })
        .collect()
}

/// Render selected attributes through LangPrint at the given declaration site.
///
/// Empty rule entries produce an empty vector. Values remain in their authored
/// order, and LangPrint supplies every language-specific wrapper.
pub(crate) fn render_attributes(
    lang: Lang,
    node: CustomizableNode,
    rules: &LanguageRules,
) -> Vec<String> {
    let language: TargetLanguage = target_language(lang);
    let attributes: Vec<RawAttribute> = raw_attributes(lang, rules);
    render_raw_attributes(language, attribute_site(node), &attributes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::LanguageAttributes;

    const LANGUAGES: [Lang; 6] = [
        Lang::Rust,
        Lang::Cpp,
        Lang::CSharp,
        Lang::Python,
        Lang::Lua,
        Lang::JsQuickJs,
    ];

    const NODES: [CustomizableNode; 10] = [
        CustomizableNode::Api,
        CustomizableNode::Type,
        CustomizableNode::Field,
        CustomizableNode::Enum,
        CustomizableNode::EnumVariant,
        CustomizableNode::GuestContract,
        CustomizableNode::HostContract,
        CustomizableNode::Function,
        CustomizableNode::Param,
        CustomizableNode::Return,
    ];

    fn rules_for(lang: Lang, values: &[&str]) -> LanguageRules {
        let attributes: LanguageAttributes = LanguageAttributes {
            attributes: values
                .iter()
                .map(|value: &&str| (*value).to_owned())
                .collect(),
        };
        match lang {
            Lang::Rust => LanguageRules {
                rust: Some(attributes),
                ..LanguageRules::default()
            },
            Lang::Cpp => LanguageRules {
                cpp: Some(attributes),
                ..LanguageRules::default()
            },
            Lang::CSharp => LanguageRules {
                csharp: Some(attributes),
                ..LanguageRules::default()
            },
            Lang::Python => LanguageRules {
                python: Some(attributes),
                ..LanguageRules::default()
            },
            Lang::Lua => LanguageRules {
                lua: Some(attributes),
                ..LanguageRules::default()
            },
            Lang::JsQuickJs => LanguageRules {
                javascript: Some(attributes),
                ..LanguageRules::default()
            },
        }
    }

    fn expected_line(language: TargetLanguage, site: AttributeSite, value: &str) -> String {
        match language {
            TargetLanguage::Rust => match site {
                AttributeSite::Root => format!("#![{value}]"),
                AttributeSite::Module
                | AttributeSite::Type
                | AttributeSite::Field
                | AttributeSite::Enum
                | AttributeSite::Variant
                | AttributeSite::Function
                | AttributeSite::Parameter
                | AttributeSite::Return => format!("#[{value}]"),
            },
            TargetLanguage::Cpp => match site {
                AttributeSite::Root => format!("// [[langprint::root({value})]]"),
                AttributeSite::Module
                | AttributeSite::Type
                | AttributeSite::Field
                | AttributeSite::Enum
                | AttributeSite::Variant
                | AttributeSite::Function
                | AttributeSite::Parameter
                | AttributeSite::Return => format!("[[{value}]]"),
            },
            TargetLanguage::CSharp => match site {
                AttributeSite::Root => format!("[assembly: {value}]"),
                AttributeSite::Module => format!("[module: {value}]"),
                AttributeSite::Return => format!("[return: {value}]"),
                AttributeSite::Type
                | AttributeSite::Field
                | AttributeSite::Enum
                | AttributeSite::Variant
                | AttributeSite::Function
                | AttributeSite::Parameter => format!("[{value}]"),
            },
            TargetLanguage::Python => match site {
                AttributeSite::Type | AttributeSite::Enum | AttributeSite::Function => {
                    format!("@{value}")
                }
                AttributeSite::Root
                | AttributeSite::Module
                | AttributeSite::Field
                | AttributeSite::Variant
                | AttributeSite::Parameter
                | AttributeSite::Return => format!("# @langprint {site:?}: {value}"),
            },
            TargetLanguage::Lua => format!("---@langprint {site:?}: {value}"),
            TargetLanguage::Js => format!("/** @langprint {site:?}: {value} */"),
        }
    }

    #[test]
    fn maps_every_polyplug_language_to_its_langprint_target() {
        let cases: [(Lang, TargetLanguage); 6] = [
            (Lang::Rust, TargetLanguage::Rust),
            (Lang::Cpp, TargetLanguage::Cpp),
            (Lang::CSharp, TargetLanguage::CSharp),
            (Lang::Python, TargetLanguage::Python),
            (Lang::Lua, TargetLanguage::Lua),
            (Lang::JsQuickJs, TargetLanguage::Js),
        ];

        for (lang, expected) in cases {
            assert_eq!(target_language(lang), expected);
        }
    }

    #[test]
    fn maps_every_customizable_node_to_its_attribute_site() {
        let cases: [(CustomizableNode, AttributeSite); 10] = [
            (CustomizableNode::Api, AttributeSite::Root),
            (CustomizableNode::Type, AttributeSite::Type),
            (CustomizableNode::Field, AttributeSite::Field),
            (CustomizableNode::Enum, AttributeSite::Enum),
            (CustomizableNode::EnumVariant, AttributeSite::Variant),
            (CustomizableNode::GuestContract, AttributeSite::Type),
            (CustomizableNode::HostContract, AttributeSite::Type),
            (CustomizableNode::Function, AttributeSite::Function),
            (CustomizableNode::Param, AttributeSite::Parameter),
            (CustomizableNode::Return, AttributeSite::Return),
        ];

        for (node, expected) in cases {
            assert_eq!(attribute_site(node), expected);
        }
    }

    #[test]
    fn renders_every_language_at_every_customizable_node_without_prewrapped_syntax() {
        for lang in LANGUAGES {
            let rules: LanguageRules = rules_for(lang, &["first(value)", "second"]);
            let language: TargetLanguage = target_language(lang);

            for node in NODES {
                let raw: Vec<RawAttribute> = raw_attributes(lang, &rules);
                assert_eq!(
                    raw,
                    vec![
                        RawAttribute {
                            source: language,
                            text: "first(value)".to_owned(),
                        },
                        RawAttribute {
                            source: language,
                            text: "second".to_owned(),
                        },
                    ]
                );
                assert!(raw.iter().all(|attribute: &RawAttribute| {
                    !attribute.text.starts_with("#[")
                        && !attribute.text.starts_with("[[")
                        && !attribute.text.starts_with('[')
                        && !attribute.text.starts_with('@')
                }));

                let expected: Vec<String> = ["first(value)", "second"]
                    .iter()
                    .map(|value: &&str| expected_line(language, attribute_site(node), value))
                    .collect();
                assert_eq!(render_attributes(lang, node, &rules), expected);
            }
        }
    }

    #[test]
    fn selects_only_requested_language_and_keeps_empty_rules_empty() {
        let rules: LanguageRules = LanguageRules {
            rust: Some(LanguageAttributes {
                attributes: vec!["derive(Clone)".to_owned(), "repr(C)".to_owned()],
            }),
            csharp: Some(LanguageAttributes {
                attributes: vec!["StructLayout(LayoutKind.Sequential)".to_owned()],
            }),
            ..LanguageRules::default()
        };

        assert_eq!(
            inner_attributes(Lang::Rust, &rules),
            ["derive(Clone)", "repr(C)"]
        );
        assert_eq!(
            raw_attributes(Lang::CSharp, &rules),
            vec![RawAttribute {
                source: TargetLanguage::CSharp,
                text: "StructLayout(LayoutKind.Sequential)".to_owned(),
            }]
        );
        assert!(inner_attributes(Lang::Lua, &rules).is_empty());
        assert!(raw_attributes(Lang::Lua, &rules).is_empty());
        assert!(render_attributes(Lang::Lua, CustomizableNode::Type, &rules).is_empty());
    }
}
