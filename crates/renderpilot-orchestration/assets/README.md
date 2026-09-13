# Bundled catalog snapshots

`reshade-v1-fallback.json` is a release-pinned, mechanically copied snapshot of
`renderpilot-libraries/addons/v1/reshade.json`. It is used only after the CDN
and every parseable cache copy are unavailable.

The snapshot is not a second live source and must never be merged into a valid
remote document. Refresh it when shipping an application release, or when the
wire schema or source security policy changes; it need not follow every CDN
refresh between releases.

## Release gate: CDN catalog paths

Before shipping an application release, confirm these remote documents are
published and fetchable (CDN + cache warm path):

| Catalog | Remote path | Bundled fallback |
| --- | --- | --- |
| Shared ReShade sources | `addons/v1/reshade.json` | `reshade-v1-fallback.json` (last resort) |
| Luma tool catalog | `addons/v1/luma.json` | **none** — install/update hard-fail if unresolved |
| RenoDX tool catalog | `addons/v1/renodx.json` | **none** |
| OptiScaler release manifest | `addons/v1/optiscaler.json` | `optiscaler-fallback.json` (validated fallback) |
| OptiScaler compatibility | `addons/v1/optiscaler-compatibility.json` | `optiscaler-compatibility-fallback.json` (validated fallback) |

Luma and RenoDX tool catalogs intentionally have no bundled offline fallback:
a stale or invented profile set is worse than a clear fetch failure. OptiScaler
uses release-pinned snapshots and last-admitted validated data because its
manifest and compatibility stores verify immutable history before accepting a
remote update. Keep every remote document current on the CDN for each release
that ships these features.

When the shared ReShade document cannot be loaded, the app logs a warning and
uses the release-pinned bundled snapshot for host downloads only.
