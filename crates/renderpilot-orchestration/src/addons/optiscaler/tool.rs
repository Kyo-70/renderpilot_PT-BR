//! OptiScaler registration in the add-on extension surface.

use std::path::Path;

use renderpilot_domain::AddonKind;

use crate::addons::capabilities::{CapabilityProbe, CapabilityProbeFuture};
use crate::addons::tool::AddonTool;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OptiScalerTool;

/// Proxy names supported by upstream releases or typed compatibility rules.
/// Detection still requires an immutable manifest hash; a matching name alone
/// never grants ownership of an existing game file.
pub(crate) const PROXY_NAMES: &[&str] = &[
    "OptiScaler.dll",
    "OptiScaler.asi",
    "nvngx.dll",
    "dxgi.dll",
    "winmm.dll",
    "d3d12.dll",
    "version.dll",
    "dbghelp.dll",
    "wininet.dll",
    "winhttp.dll",
];

pub(crate) fn capability_probe(
    catalog: super::compatibility_catalog::OptiScalerCompatibilityCatalog,
) -> CapabilityProbe {
    let revision = catalog.revision.clone();
    CapabilityProbe::new(AddonKind::OptiScaler, revision, move |facts, components| {
        super::matcher::capability_available(&catalog, facts, components)
    })
}

impl AddonTool for OptiScalerTool {
    fn kind(&self) -> AddonKind {
        AddonKind::OptiScaler
    }

    fn exclusive_peers(&self) -> &'static [AddonKind] {
        &[]
    }

    fn exclusive_block_message(&self, unmanaged: bool) -> &'static str {
        if unmanaged {
            "an unmanaged proxy conflicts with OptiScaler"
        } else {
            "a managed proxy topology conflicts with OptiScaler"
        }
    }

    fn unmanaged_present(&self, dir: &Path) -> bool {
        unmanaged_install_present(dir)
    }

    fn finalizing_phase(&self) -> &'static str {
        super::PHASE_FINALIZING
    }

    fn load_capability_probe(&self) -> CapabilityProbeFuture {
        Box::pin(async {
            let catalog = super::compatibility_catalog::get_or_fetch_catalog().await?;
            Ok(capability_probe(catalog))
        })
    }
}

/// A configuration file is not an installation by itself. RenderPilot may
/// legitimately leave a recovery source behind after an older failed removal,
/// and upstream users also share INI presets independently of the runtime.
/// Require a plausible OptiScaler loader/runtime alongside it before entering
/// exact, manifest-verified automatic reconciliation.
pub(crate) fn unmanaged_install_present(dir: &Path) -> bool {
    let has_config = dir.join("OptiScaler.ini").is_file();
    let has_proxy = PROXY_NAMES.iter().any(|name| {
        let path = dir.join(name);
        path.is_file() && !crate::addons::reshade::scan::is_reshade_proxy_file(&path)
    });
    let private_runtime = dir.join("OptiScaler");
    let has_private_runtime = private_runtime.is_dir()
        && std::fs::read_dir(private_runtime).is_ok_and(|mut entries| entries.next().is_some());
    has_config && (has_proxy || has_private_runtime)
}

#[cfg(test)]
mod tests {
    use super::unmanaged_install_present;

    #[test]
    fn standalone_recovered_config_is_not_an_unmanaged_install() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("OptiScaler.ini"), b"[OptiScaler]\n")
            .expect("config fixture");
        assert!(!unmanaged_install_present(dir.path()));

        let reshade = crate::addons::test_support::build_pe_with_exports(
            crate::addons::test_support::MACHINE_AMD64,
            crate::addons::test_support::PE32_PLUS_MAGIC,
            &["ReShadeVersion"],
        );
        std::fs::write(dir.path().join("dxgi.dll"), reshade).expect("ReShade fixture");
        assert!(
            !unmanaged_install_present(dir.path()),
            "a standalone preset beside managed ReShade is not OptiScaler"
        );

        std::fs::write(dir.path().join("dxgi.dll"), b"proxy").expect("proxy fixture");
        assert!(unmanaged_install_present(dir.path()));
    }
}
