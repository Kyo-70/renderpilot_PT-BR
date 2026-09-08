use super::super::super::*;
use super::context::BindingContext;
use std::collections::BTreeMap;

pub(super) fn collect_retained_claims<'claims>(
    context: &mut BindingContext<'_>,
    retained_claims: &'claims [OptiScalerRetainedClaim],
) -> AppResult<BTreeMap<String, &'claims OptiScalerRetainedClaim>> {
    retained_claims.iter().try_fold(
        BTreeMap::new(),
        |mut claims: BTreeMap<String, &'claims OptiScalerRetainedClaim>, claim| {
            let key = normalized_path_key(claim.path.as_str());
            if claims.insert(key.clone(), claim).is_some() {
                return Err(renderpilot_application::AppError::invalid_input(
                    "OptiScaler retained claims contain a duplicate normalized path",
                ));
            }
            let receipt = context
                .before_receipts
                .get(&key)
                .or_else(|| context.after_receipts.get(&key))
                .map(|(_, receipt)| receipt)
                .ok_or_else(|| {
                    renderpilot_application::AppError::invalid_input(
                        "OptiScaler retained claim has no exact topology receipt",
                    )
                })?;
            // A remaining peer carries preservation authority, not
            // OptiScaler's destructive ownership. The handoff therefore
            // intentionally changes only Owned -> Reused; native identity
            // and content digest must remain exact.
            if !is_reused(&claim.receipt)
                || !same_receipt_identity(receipt, &claim.receipt)
                || receipt.digest() != claim.receipt.digest()
            {
                return Err(renderpilot_application::AppError::invalid_input(
                    "OptiScaler retained claim does not preserve the exact identity and digest as reused custody",
                ));
            }
            context.known_paths.insert(key);
            Ok(claims)
        },
    )
}
