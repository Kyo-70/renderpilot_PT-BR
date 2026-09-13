# RenoDX, Luma, and OptiScaler

RenderPilot can manage supported RenoDX, Luma, and OptiScaler installations. These are independent third-party projects with their own licenses, compatibility boundaries, update channels, and risks. ReShade may be installed as a shared host. Read the upstream documentation for the selected profile before modifying a game.

## RenoDX

RenoDX profiles can add HDR and related rendering controls to supported games. RenderPilot evaluates the selected game, displays match confidence and host requirements, and prepares the required RenoDX and ReShade files. Some profiles can use an optional DLSS Frame Generation fix so the ReShade output is handled correctly with generated frames.

Where a profile is not distributed from an automatically supported source, RenderPilot can validate a file you downloaded yourself through the file picker or drag-and-drop flow. That path does not make an unknown archive trusted: obtain it from the profile's official distribution channel and verify its instructions.

## Luma

Luma profiles can provide DirectX 11 upscaling, HDR, and shader replacement for supported games. New managed installations use the nightly ReShade host. Their requirements can include a particular host layout, Microsoft Visual C++ runtime, launch arguments, DLSS bindings, or dgVoodoo components. RenderPilot reports the dependencies and planned files that apply to the chosen title.

Luma and RenoDX are treated as mutually exclusive managed add-ons for a game. Remove the active one before installing the other. Shared ReShade files are tracked so removal can distinguish an add-on's managed files from files that another supported feature still needs.

## OptiScaler

OptiScaler adapts supported upscaling and frame-generation inputs for its managed runtime. Its compatibility catalog provides exact game guidance, declared inputs, launch requirements, and prerequisites; it is advisory for visibility, not an allowlist. Game Details shows OptiScaler for every x64 game executable using DirectX 11, DirectX 12, or Vulkan, including games that are not yet in the catalog. An explicit catalog entry marked unsupported remains blocked and is not offered as a fresh capability.

An unknown configuration with a detected DLSS 2+, FSR 2+, or XeSS input can be installed directly. An unknown configuration without one of those inputs remains visible so its state is clear, but installation is blocked until a supported input is detected. OptiScaler does not offer a compatibility confirmation or expert override for either case.

OptiScaler installations can be configured, updated, repaired, relocated when the selected executable changes, or uninstalled. Installed state remains available for maintenance and cleanup even if a later catalog refresh changes the current compatibility result. A remote catalog failure keeps the last admitted or bundled fallback data where that source's contract permits it.

## Updates and removal

Status and update checks use current manifests and, when supported, the add-on's upstream source. A failed remote check can leave the last known information available, but it should not be mistaken for proof that no update exists. Removal previews and reverses files tracked by RenderPilot; unrelated files are not intentionally deleted.

Installing, updating, or repairing an add-on changes files in the selected game and therefore participates in RenderPilot's general [game-file safety](game-file-safety.md) flow. Add-on removal remains available so managed files can be restored even when a fresh safety context cannot be acquired.

## Sources of truth

- [RenoDX orchestration](../../crates/renderpilot-orchestration/src/addons/renodx/mod.rs)
- [Luma orchestration](../../crates/renderpilot-orchestration/src/addons/luma/mod.rs)
- [OptiScaler orchestration](../../crates/renderpilot-orchestration/src/addons/optiscaler/mod.rs)
- [OptiScaler compatibility matching](../../crates/renderpilot-orchestration/src/addons/optiscaler/matcher/capability.rs)
