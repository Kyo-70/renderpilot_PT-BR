import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking: '安装 Luma 期间请勿使用官方公开匹配，否则可能导致封禁。',
  deusExBorisEnb:
    '与 Boris ENB (DX9) 不兼容。兼容导剪版 (Director’s Cut) 和原版。使用 Luma 时无需 Gold Filter Restoration 模组。',
  dlssOnlyNoHdr: '此配置文件仅支持 DLSS，目前不支持 HDR。',
  hatsuneExclusiveFullscreen: '若出现显示异常，请避免使用独占全屏。按 Alt+Enter 可切换窗口模式。',
  heavyRainSteamUltrawide: '超宽屏支持可能仅在 Steam 上有效。',
  xboxStore: '与 Xbox Store 版本不兼容。',
  metroWindowed: '需要窗口化或无边框模式，可通过模组或在游戏配置文件中关闭全屏进行设置。',
  metroBorderless: '请使用无边框窗口模式。',
  preyData: '请将 Prey 的附加 Luma 数据文件与插件放置在同一目录下。',
  massEffectNativeAa:
    '仅提供 DLAA / FSR 3 原生抗锯齿 (Native AA) 模式；这不是 DLSS 或 FSR 超分辨率。',
  manualLaunchArgument: '请手动添加此启动参数。',
  aceFxaaHigh: '在游戏设置中选择：AA “FXAA High”。',
  manualEngineIni: '请在 Engine.ini 中手动应用以下设置。',
  callSeaEpic: '在游戏设置中将整体质量设为 “Epic”。',
  codeVeinAaHighest: '在游戏设置中选择 AA Highest。',
  crabHighAntialiasing: '在游戏设置中选择 High Anti-aliasing Type。',
  crashAaMedium: '在游戏设置中选择：至少 Medium (2x) 的抗锯齿质量。',
  clashAaVeryHigh: '在游戏设置中将 AA 质量设为 “Very High”。',
  closeToSunAa4x: '在游戏设置中选择 AA 4X。',
  darksidersAaEpic: '在游戏设置中选择 AA Epic。',
  daymareOptiscalerUuu: 'Luma 可单独运行，但与 OptiScaler 或 UUU 配合使用时会发生崩溃。',
  deadlineUltra: '在游戏设置中选择 “Ultra”。',
  dieYoungTaa: '在游戏设置中选择 TAA “High” 或 “Epic”。',
  dnfCharacterSelection: '抗锯齿在角色选择界面无效。',
  kakarotBdzKfix: '在游戏设置中选择：TAA。原版 (Legacy) 请使用 BDZKFix，HD 版请使用其更新分支。',
  filamentAaHigh: '在游戏设置中选择 AA “High” 或 “Very High”。',
  goatHighAa: '在游戏设置中选择 High AA。',
  guiltyGearStriveAa:
    '抗锯齿在角色选择界面无效。游戏内设置：AA “Temporal Anti Aliasing”。在 Engine.ini 的 [SystemSettings] 下添加：r.DefaultFeature.AntiAliasing=2 与 r.PostProcessAAQuality=4。',
  itTakesTwoTitle: '仅在标题画面过场期间有效。',
  aaHigh: '在游戏设置中选择 AA “High”。',
  kh3Txaa: '在游戏设置中选择：“TXAA”。',
  mutantMotionBlur:
    '在游戏设置中选择：AA “High”。为了获得更清晰的动态画面，建议在 Engine.ini 中设置 r.motionblur.amount=0。',
  orcsAaHigh: '在游戏设置中将 AA 质量设为 “High”。',
  projectWingmanFxaa: '在游戏设置中选择：AA “FXAA”。',
  scarletNexusTxaa: '在游戏设置中选择 AA “TXAA”。',
  scornOptiscaler: '原生支持 FSR 2.1；可通过 OptiScaler 添加 DLSS 或其他缩放器。',
  smtLyallFix: '需要 Lyall’s Fix 强制开启 TAA。',
  spiritNorthUltra: '在游戏设置中将图形质量设为 “Ultra”。',
  spyroHighTaa: '在游戏设置中选择 High TAA。',
  supralandTaa: '在游戏设置中选择 AA “Temporal Anti Aliasing”。',
  talesAriseSdk: '需要 Arise-SDK 并设置 UseUE4TAA=true。',
  tekkenNoD3D9Ex: '需要启动参数 -nod3d9ex。',
  tetrisFxaa6: '在游戏设置中选择：AA “FXAA:6” 且渲染比例为 100%。',
  sinkingCityOriginal: '兼容原版；重制版 (Remastered) 的兼容性尚未确认。',
  vampyrTxaa6x: '在游戏设置中选择 AA TXAA 6X。',
  edithFinchExit:
    'DLAA 无需额外修改即可运行，但退出后游戏可能无法完全关闭。OptiScaler 可以解决此问题。',
  edithFinch4k:
    '该游戏在 4K 分辨率下运行不稳定。在手动应用 Engine.ini 设置前，请将 Effects 设为 Low。',
  sherlockDx11Performance:
    '启动参数 -dx11 会导致 CPU 性能下降。在启用 Auto Exposure 的 DLAA 下，草地上会出现锯齿边缘。',
  fallout4DlssGtaoOnly: '此配置文件目前仅支持 DLSS 和 GTAO。',
  biomutantAaHighOrMax: '在游戏设置中选择 AA “High” 或 “Max”。',
  blairWitchTxaaFull: '在游戏设置中选择：TXAA 以及分辨率缩放 “Full”。',
  flickeringIssues: '可能会出现画面闪烁。',
  brambleEpicVram:
    'Epic 画质会持续占用显存 (VRAM) 并导致卡顿。在 Luma 运行时，请避免在 High 和 Epic 之间频繁切换。',
  daemonDlaaReset: '加载关卡或更改图形设置会强制设为 r.TemporalAASamples=1 并禁用 DLAA。',
  easyAntiCheatBlocked: '被 Easy Anti-Cheat 拦截。',
  echoDlaaAutoExposure:
    '第一关后 DLAA 将停止工作。启用 Auto Exposure 时光源会出现频闪；禁用 Auto Exposure 时抗锯齿质量会明显下降。',
  dx11BootFailure: '无法在 DirectX 11 模式下启动。',
  rainCodeAaHighMaxResolution: '在游戏设置中选择：AA 质量 “High” 并将分辨率滑块拉至最大。',
  roboquestTaaQuality3: '在游戏设置中选择 TAA，并将质量设为 “3”。',
  aaUltra: '在游戏设置中选择 AA “Ultra”。',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'zh-Hans', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
