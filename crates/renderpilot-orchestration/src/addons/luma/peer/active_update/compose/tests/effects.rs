use std::path::Path;

use crate::addons::luma::peer::effects::{
    LumaPeerEffectAccumulator, LumaPeerEffectGroup, LumaPeerOperationOrder,
};

use super::path;

#[test]
fn finalization_orders_groups_and_keeps_payloads_aligned() {
    let root = Path::new("C:/Games/Test");
    let mut accumulator = LumaPeerEffectAccumulator::new(LumaPeerOperationOrder::InstallOrUpdate);
    accumulator
        .create(LumaPeerEffectGroup::Host, path(root, "host.dll"), vec![4])
        .expect("host");
    accumulator
        .create(
            LumaPeerEffectGroup::DlssCascade,
            path(root, "dlss.dll"),
            vec![3],
        )
        .expect("dlss");
    accumulator
        .create(
            LumaPeerEffectGroup::DgVoodoo,
            path(root, "dgvoodoo.dll"),
            vec![2],
        )
        .expect("dgvoodoo");
    accumulator
        .create(
            LumaPeerEffectGroup::Generic,
            path(root, "addon.addon64"),
            vec![1],
        )
        .expect("generic");

    let effects = accumulator
        .finalize()
        .expect("finalize")
        .expect("physical effects");
    let paths = effects
        .program()
        .endpoints()
        .iter()
        .map(|endpoint| endpoint.path().as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        vec![
            "C:/Games/Test/addon.addon64",
            "C:/Games/Test/dgvoodoo.dll",
            "C:/Games/Test/dlss.dll",
            "C:/Games/Test/host.dll",
        ]
    );
    assert_eq!(
        effects.payloads(),
        &[Some(vec![1]), Some(vec![2]), Some(vec![3]), Some(vec![4])]
    );
}
