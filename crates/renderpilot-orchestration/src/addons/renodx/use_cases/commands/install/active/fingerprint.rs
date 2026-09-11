use renderpilot_domain::{Architecture, Sha256Hash};
use sha2::{Digest, Sha256};

use crate::ServiceError;
use crate::addons::renodx::errors;
use crate::addons::renodx::matcher::ResolvedInstall;
use crate::addons::reshade::proxy::HostKind;
use crate::addons::reshade::types::ReshadeChannel;

use super::model::ActiveInstallSource;

const REQUEST_DOMAIN: &[u8] = b"renderpilot.renodx.active.request.v1";
const PLAN_DOMAIN: &[u8] = b"renderpilot.renodx.active.plan.v1";

/// Computes the canonical identity of the public active-install request.
pub(super) fn request_fingerprint(
    game_id: &renderpilot_domain::GameId,
    source: ActiveInstallSource,
    channel: ReshadeChannel,
) -> Result<Sha256Hash, ServiceError> {
    let mut builder = FingerprintBuilder::new(REQUEST_DOMAIN);
    builder.field(game_id.as_str().as_bytes());
    builder.field(source.stable_label().as_bytes());
    builder.field(source.reshade_ini_feature().as_feature().as_bytes());
    builder.field(channel.as_str().as_bytes());
    if let Some(architecture) = source.file_architecture() {
        builder.field(architecture_label(architecture).as_bytes());
    }
    builder.finish()
}

/// Computes the canonical identity of the resolved plan.
pub(super) fn plan_fingerprint(plan: &ResolvedInstall) -> Result<Sha256Hash, ServiceError> {
    let mut builder = FingerprintBuilder::new(PLAN_DOMAIN);
    builder.field(plan.slug.as_bytes());
    builder.field(plan.addon_url.as_bytes());
    builder.field(architecture_label(plan.arch).as_bytes());
    builder.field(host_kind_label(plan.host_kind).as_bytes());
    builder.field(plan.proxy_dll_name.as_bytes());
    builder.finish()
}

pub(super) fn architecture_label(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X64 => "x64",
        Architecture::X86 => "x86",
    }
}

fn host_kind_label(host_kind: HostKind) -> &'static str {
    match host_kind {
        HostKind::Proxy => "proxy",
        HostKind::Vulkan => "vulkan",
    }
}

struct FingerprintBuilder {
    hasher: Sha256,
}

impl FingerprintBuilder {
    fn new(domain: &[u8]) -> Self {
        let mut builder = Self {
            hasher: Sha256::new(),
        };
        builder.field(domain);
        builder
    }

    fn field(&mut self, bytes: &[u8]) {
        self.hasher.update((bytes.len() as u64).to_le_bytes());
        self.hasher.update(bytes);
    }

    fn finish(self) -> Result<Sha256Hash, ServiceError> {
        Sha256Hash::new(hex::encode(self.hasher.finalize())).map_err(|error| {
            errors::failed(format!("active RenoDX fingerprint is invalid: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addons::matching::MatchConfidence;

    fn plan(
        slug: &str,
        url: &str,
        arch: Architecture,
        host_kind: HostKind,
        proxy_dll_name: &str,
    ) -> ResolvedInstall {
        ResolvedInstall {
            slug: slug.to_owned(),
            addon_url: url.to_owned(),
            arch,
            host_kind,
            proxy_dll_name: proxy_dll_name.to_owned(),
            confidence: MatchConfidence::Untested,
            generic_profile: None,
        }
    }

    #[test]
    fn field_framing_distinguishes_concatenation_boundaries() {
        let mut left = FingerprintBuilder::new(b"test");
        left.field(b"ab");
        left.field(b"c");
        let mut right = FingerprintBuilder::new(b"test");
        right.field(b"a");
        right.field(b"bc");
        assert_ne!(left.finish().expect("left"), right.finish().expect("right"));
    }

    #[test]
    fn request_fingerprint_tracks_source_channel_and_local_architecture() {
        let game = renderpilot_domain::GameId::new("manual:fingerprint").expect("game id");
        let catalog =
            request_fingerprint(&game, ActiveInstallSource::Catalog, ReshadeChannel::Stable)
                .expect("catalog fingerprint");
        assert_ne!(
            catalog,
            request_fingerprint(&game, ActiveInstallSource::Catalog, ReshadeChannel::Nightly)
                .expect("nightly fingerprint")
        );
        assert_ne!(
            catalog,
            request_fingerprint(
                &game,
                ActiveInstallSource::InstallFromFile {
                    architecture: Architecture::X64,
                },
                ReshadeChannel::Stable,
            )
            .expect("file fingerprint")
        );
        assert_ne!(
            request_fingerprint(
                &game,
                ActiveInstallSource::InstallFromFile {
                    architecture: Architecture::X64,
                },
                ReshadeChannel::Stable,
            )
            .expect("x64 fingerprint"),
            request_fingerprint(
                &game,
                ActiveInstallSource::InstallFromFile {
                    architecture: Architecture::X86,
                },
                ReshadeChannel::Stable,
            )
            .expect("x86 fingerprint")
        );
    }

    #[test]
    fn plan_fingerprint_tracks_each_plan_identity_field() {
        let base = plan(
            "game",
            "https://example.test/a",
            Architecture::X64,
            HostKind::Proxy,
            "dxgi.dll",
        );
        let variants = [
            plan(
                "other",
                &base.addon_url,
                base.arch,
                base.host_kind,
                &base.proxy_dll_name,
            ),
            plan(
                &base.slug,
                "https://example.test/b",
                base.arch,
                base.host_kind,
                &base.proxy_dll_name,
            ),
            plan(
                &base.slug,
                &base.addon_url,
                Architecture::X86,
                base.host_kind,
                &base.proxy_dll_name,
            ),
            plan(
                &base.slug,
                &base.addon_url,
                base.arch,
                HostKind::Vulkan,
                &base.proxy_dll_name,
            ),
            plan(
                &base.slug,
                &base.addon_url,
                base.arch,
                base.host_kind,
                "d3d12.dll",
            ),
        ];
        let expected = plan_fingerprint(&base).expect("base fingerprint");
        for variant in variants {
            assert_ne!(
                expected,
                plan_fingerprint(&variant).expect("variant fingerprint")
            );
        }
    }

    #[test]
    fn request_fingerprint_tracks_game_identity() {
        let first = renderpilot_domain::GameId::new("manual:fingerprint-a").expect("first game");
        let second = renderpilot_domain::GameId::new("manual:fingerprint-b").expect("second game");
        assert_ne!(
            request_fingerprint(&first, ActiveInstallSource::Catalog, ReshadeChannel::Stable)
                .expect("first fingerprint"),
            request_fingerprint(
                &second,
                ActiveInstallSource::Catalog,
                ReshadeChannel::Stable
            )
            .expect("second fingerprint")
        );
    }
}
