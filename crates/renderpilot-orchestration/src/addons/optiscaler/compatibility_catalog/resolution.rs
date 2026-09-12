use crate::addons::matching::MatchFacts;

use super::{
    CompatibilityEntry, EntryIdentityKind, OptiScalerCompatibilityCatalog, ResolvedCompatibility,
};

/// Resolves exact game knowledge.  An identity collision is a publisher error,
/// but an independently parsed malformed remote cache must still fail closed at
/// runtime rather than choosing by order.
pub(crate) fn resolve<'a>(
    catalog: &'a OptiScalerCompatibilityCatalog,
    facts: &MatchFacts,
) -> ResolvedCompatibility<'a> {
    let mut matches = catalog
        .entries()
        .iter()
        .filter(|entry| entry_matches(entry, facts));
    let entry = match (matches.next(), matches.next()) {
        (None, _) => return ResolvedCompatibility::NoMatch,
        (Some(_), Some(_)) => return ResolvedCompatibility::Conflict,
        (Some(entry), None) => entry,
    };
    let mut variant_matches = entry.variants.iter().filter(|variant| {
        variant
            .condition
            .as_ref()
            .is_some_and(|condition| condition_matches(condition, facts))
    });
    let variant = match (variant_matches.next(), variant_matches.next()) {
        (None, _) => entry
            .variants
            .iter()
            .find(|variant| variant.condition.is_none()),
        (Some(variant), None) => Some(variant),
        (Some(_), Some(_)) => None,
    };
    match variant {
        Some(variant) => ResolvedCompatibility::Match {
            entry,
            variant: &variant.resolved,
        },
        None => ResolvedCompatibility::Conflict,
    }
}

fn entry_matches(entry: &CompatibilityEntry, facts: &MatchFacts) -> bool {
    entry.identities.iter().any(|identity| match identity.kind {
        EntryIdentityKind::SteamAppid => {
            facts.launcher == renderpilot_domain::Launcher::Steam
                && facts.external_id.as_deref() == Some(identity.value.as_str())
        }
        EntryIdentityKind::EpicId => {
            facts.launcher == renderpilot_domain::Launcher::Epic
                && facts.external_id.as_deref() == Some(identity.value.as_str())
        }
        EntryIdentityKind::GogId => {
            facts.launcher == renderpilot_domain::Launcher::Gog
                && facts.external_id.as_deref() == Some(identity.value.as_str())
        }
        EntryIdentityKind::XboxStoreId => {
            facts.launcher == renderpilot_domain::Launcher::Xbox
                && facts.external_id.as_deref().is_some_and(|actual| {
                    let trimmed = actual.trim();
                    trimmed.len() == 12
                        && trimmed.bytes().all(|byte| byte.is_ascii_alphanumeric())
                        && trimmed.eq_ignore_ascii_case(&identity.value)
                })
        }
        EntryIdentityKind::ExeName => facts
            .exe_file_name
            .as_deref()
            .is_some_and(|actual| actual.eq_ignore_ascii_case(&identity.value)),
    })
}

fn condition_matches(
    condition: &super::model::ValidatedVariantCondition,
    facts: &MatchFacts,
) -> bool {
    (condition
        .launcher
        .is_none_or(|launcher| launcher.matches(facts.launcher)))
        && condition.executable.as_deref().is_none_or(|expected| {
            facts
                .exe_file_name
                .as_deref()
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        })
}

#[cfg(test)]
mod tests {
    use renderpilot_domain::{Architecture, ExeGraphicsInfo, GraphicsApi, Launcher};

    use super::*;
    use crate::addons::optiscaler::compatibility_catalog::model::{
        CompatibilityPrerequisite, CompatibilityStatus, EntryIdentity, EntryIdentityKind,
        OptiPatcherPolicy, ProxyPolicy, WindowsLauncher, WireCompatibilityCatalog,
        WireCompatibilityEntry, WireCompatibilityVariant, WireUpstream, WireVariantCondition,
    };

    fn facts(
        launcher: Launcher,
        external_id: Option<&str>,
        executable: Option<&str>,
    ) -> MatchFacts {
        MatchFacts {
            launcher,
            external_id: external_id.map(str::to_owned),
            exe_file_name: executable.map(str::to_owned),
            engine: None,
            graphics: ExeGraphicsInfo::new(vec![GraphicsApi::D3D12], Some(Architecture::X64)),
        }
    }

    fn variant(
        condition: Option<WireVariantCondition>,
        proxy: ProxyPolicy,
    ) -> WireCompatibilityVariant {
        WireCompatibilityVariant {
            when: condition,
            proxy,
            ini_overrides: Vec::new(),
            launch: None,
            restricted_modules: Vec::new(),
            optipatcher: OptiPatcherPolicy::Unspecified,
            prerequisite: CompatibilityPrerequisite::None,
        }
    }

    fn catalog(entries: Vec<WireCompatibilityEntry>) -> OptiScalerCompatibilityCatalog {
        WireCompatibilityCatalog {
            schema_version: 1,
            revision: "2026-09-10.1".to_owned(),
            upstream: WireUpstream {
                source: "test".to_owned(),
                snapshot_revision: "test".to_owned(),
                snapshot_sha256: "0".repeat(64),
            },
            entries,
        }
        .try_into()
        .expect("test catalog")
    }

    fn entry(
        id: &str,
        identities: Vec<EntryIdentity>,
        variants: Vec<WireCompatibilityVariant>,
    ) -> WireCompatibilityEntry {
        WireCompatibilityEntry {
            id: id.to_owned(),
            status: CompatibilityStatus::Working,
            identities,
            declared_inputs: Vec::new(),
            guidance: Vec::new(),
            variants,
        }
    }

    fn resolved_proxy<'catalog>(
        catalog: &'catalog OptiScalerCompatibilityCatalog,
        facts: &MatchFacts,
    ) -> Option<&'catalog ProxyPolicy> {
        match resolve(catalog, facts) {
            ResolvedCompatibility::Match { variant, .. } => Some(&variant.proxy),
            ResolvedCompatibility::NoMatch | ResolvedCompatibility::Conflict => None,
        }
    }

    #[test]
    fn manual_exact_executable_uses_its_entry_default_not_an_xbox_override() {
        let catalog = catalog(vec![entry(
            "exact-executable",
            vec![EntryIdentity {
                kind: EntryIdentityKind::ExeName,
                value: "Game.exe".to_owned(),
            }],
            vec![
                variant(
                    None,
                    ProxyPolicy::Exact {
                        slot: "version.dll".to_owned(),
                    },
                ),
                variant(
                    Some(WireVariantCondition {
                        launcher: Some(WindowsLauncher::Xbox),
                        executable: None,
                    }),
                    ProxyPolicy::Exact {
                        slot: "winmm.dll".to_owned(),
                    },
                ),
            ],
        )]);

        assert_eq!(
            resolved_proxy(&catalog, &facts(Launcher::Manual, None, Some("game.EXE"))),
            Some(&ProxyPolicy::Exact {
                slot: "version.dll".to_owned()
            })
        );
    }

    #[test]
    fn xbox_store_id_requires_a_valid_xbox_store_identity_and_selects_its_override() {
        let catalog = catalog(vec![entry(
            "xbox-store",
            vec![EntryIdentity {
                kind: EntryIdentityKind::XboxStoreId,
                value: "9MW53ZKZH168".to_owned(),
            }],
            vec![
                variant(None, ProxyPolicy::Automatic),
                variant(
                    Some(WireVariantCondition {
                        launcher: Some(WindowsLauncher::Xbox),
                        executable: None,
                    }),
                    ProxyPolicy::Exact {
                        slot: "winmm.dll".to_owned(),
                    },
                ),
            ],
        )]);

        assert_eq!(
            resolved_proxy(
                &catalog,
                &facts(Launcher::Xbox, Some("9mw53zkzh168"), Some("Game.exe")),
            ),
            Some(&ProxyPolicy::Exact {
                slot: "winmm.dll".to_owned()
            })
        );
        assert_eq!(
            resolved_proxy(
                &catalog,
                &facts(Launcher::Xbox, Some("package_family"), Some("Game.exe")),
            ),
            None,
            "a package family name is not a StoreId"
        );
        assert_eq!(
            resolved_proxy(
                &catalog,
                &facts(Launcher::Manual, Some("9MW53ZKZH168"), Some("Game.exe")),
            ),
            None,
            "a manual root cannot claim Xbox Store identity"
        );
    }

    #[test]
    fn catalog_rejects_noncanonical_xbox_store_ids_and_non_leaf_executable_identities() {
        let invalid_store = WireCompatibilityCatalog {
            schema_version: 1,
            revision: "2026-09-10.1".to_owned(),
            upstream: WireUpstream {
                source: "test".to_owned(),
                snapshot_revision: "test".to_owned(),
                snapshot_sha256: "0".repeat(64),
            },
            entries: vec![entry(
                "invalid-store",
                vec![EntryIdentity {
                    kind: EntryIdentityKind::XboxStoreId,
                    value: "9mw53zkzh168".to_owned(),
                }],
                vec![variant(None, ProxyPolicy::Automatic)],
            )],
        };
        assert!(OptiScalerCompatibilityCatalog::try_from(invalid_store).is_err());

        let invalid_executable = entry(
            "invalid-executable",
            vec![EntryIdentity {
                kind: EntryIdentityKind::ExeName,
                value: "bin/Game.exe".to_owned(),
            }],
            vec![variant(None, ProxyPolicy::Automatic)],
        );
        let wire = WireCompatibilityCatalog {
            schema_version: 1,
            revision: "2026-09-10.1".to_owned(),
            upstream: WireUpstream {
                source: "test".to_owned(),
                snapshot_revision: "test".to_owned(),
                snapshot_sha256: "0".repeat(64),
            },
            entries: vec![invalid_executable],
        };
        assert!(OptiScalerCompatibilityCatalog::try_from(wire).is_err());
    }
}
