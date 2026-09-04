import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking: '安裝 Luma 期間請勿使用官方公開配對，否則可能導致封鎖。',
  deusExBorisEnb:
    '與 Boris ENB (DX9) 不相容。相容導演剪輯版 (Director’s Cut) 和原版。使用 Luma 時無需 Gold Filter Restoration 模組。',
  dlssOnlyNoHdr: '此設定檔僅支援 DLSS，目前不支援 HDR。',
  hatsuneExclusiveFullscreen: '若出現顯示異常，請避免使用獨佔全螢幕。按 Alt+Enter 可切換視窗模式。',
  heavyRainSteamUltrawide: '超寬螢幕支援可能僅在 Steam 上有效。',
  xboxStore: '與 Xbox Store 版本不相容。',
  metroWindowed: '需要視窗化或無邊框模式，可透過模組或在遊戲設定檔中關閉全螢幕進行設定。',
  metroBorderless: '請使用無邊框視窗模式。',
  preyData: '請將 Prey 的附加 Luma 資料檔案與外掛程式放置在同一目錄下。',
  massEffectNativeAa:
    '僅提供 DLAA / FSR 3 原生反鋸齒 (Native AA) 模式；這不是 DLSS 或 FSR 超解析度。',
  manualLaunchArgument: '請手動新增此啟動參數。',
  aceFxaaHigh: '在遊戲設定中選擇：AA 「FXAA High」。',
  manualEngineIni: '請在 Engine.ini 中手動套用以下設定。',
  callSeaEpic: '在遊戲設定中將整體品質設為 「Epic」。',
  codeVeinAaHighest: '在遊戲設定中選擇 AA Highest。',
  crabHighAntialiasing: '在遊戲設定中選擇 High Anti-aliasing Type。',
  crashAaMedium: '在遊戲設定中選擇：至少 Medium (2x) 的反鋸齒品質。',
  clashAaVeryHigh: '在遊戲設定中將 AA 品質設為 「Very High」。',
  closeToSunAa4x: '在遊戲設定中選擇 AA 4X。',
  darksidersAaEpic: '在遊戲設定中選擇 AA Epic。',
  daymareOptiscalerUuu: 'Luma 可單獨運作，但與 OptiScaler 或 UUU 配合使用時會發生當機。',
  deadlineUltra: '在遊戲設定中選擇 「Ultra」。',
  dieYoungTaa: '在遊戲設定中選擇 TAA 「High」 或 「Epic」。',
  dnfCharacterSelection: '反鋸齒在角色選擇畫面無效。',
  kakarotBdzKfix: '在遊戲設定中選擇：TAA。原版 (Legacy) 請使用 BDZKFix，HD 版請使用其更新分支。',
  filamentAaHigh: '在遊戲設定中選擇 AA 「High」 或 「Very High」。',
  goatHighAa: '在遊戲設定中選擇 High AA。',
  guiltyGearStriveAa:
    '反鋸齒在角色選擇畫面無效。遊戲內設定：AA 「Temporal Anti Aliasing」。在 Engine.ini 的 [SystemSettings] 下新增：r.DefaultFeature.AntiAliasing=2 與 r.PostProcessAAQuality=4。',
  itTakesTwoTitle: '僅在標題畫面過場期間有效。',
  aaHigh: '在遊戲設定中選擇 AA 「High」。',
  kh3Txaa: '在遊戲設定中選擇：「TXAA」。',
  mutantMotionBlur:
    '在遊戲設定中選擇：AA 「High」。為了獲得更清晰的動態畫面，建議在 Engine.ini 中設定 r.motionblur.amount=0。',
  orcsAaHigh: '在遊戲設定中將 AA 品質設為 「High」。',
  projectWingmanFxaa: '在遊戲設定中選擇：AA 「FXAA」。',
  scarletNexusTxaa: '在遊戲設定中選擇 AA 「TXAA」。',
  scornOptiscaler: '原生支援 FSR 2.1；可透過 OptiScaler 新增 DLSS 或其他縮放器。',
  smtLyallFix: '需要 Lyall’s Fix 強制開啟 TAA。',
  spiritNorthUltra: '在遊戲設定中將圖形品質設為 「Ultra」。',
  spyroHighTaa: '在遊戲設定中選擇 High TAA。',
  supralandTaa: '在遊戲設定中選擇 AA 「Temporal Anti Aliasing」。',
  talesAriseSdk: '需要 Arise-SDK 並設定 UseUE4TAA=true。',
  tekkenNoD3D9Ex: '需要啟動參數 -nod3d9ex。',
  tetrisFxaa6: '在遊戲設定中選擇：AA 「FXAA:6」 且渲染比例為 100%。',
  sinkingCityOriginal: '相容原版；重製版 (Remastered) 的相容性尚未確認。',
  vampyrTxaa6x: '在遊戲設定中選擇 AA TXAA 6X。',
  edithFinchExit:
    'DLAA 無需額外修改即可運作，但結束後遊戲可能無法完全關閉。OptiScaler 可以解決此問題。',
  edithFinch4k:
    '該遊戲在 4K 解析度下運作不穩定。在手動套用 Engine.ini 設定前，請將 Effects 設為 Low。',
  sherlockDx11Performance:
    '啟動參數 -dx11 會導致 CPU 效能下降。在啟用 Auto Exposure 的 DLAA 下，草地上會出現鋸齒邊緣。',
  fallout4DlssGtaoOnly: '此設定檔目前僅支援 DLSS 和 GTAO。',
  biomutantAaHighOrMax: '在遊戲設定中選擇 AA 「High」 或 「Max」。',
  blairWitchTxaaFull: '在遊戲設定中選擇：TXAA 以及解析度縮放 「Full」。',
  flickeringIssues: '可能會出現畫面閃爍。',
  brambleEpicVram:
    'Epic 畫質會持續佔用視訊記憶體 (VRAM) 並導致卡頓。在 Luma 運作時，請避免在 High 和 Epic 之間頻繁切換。',
  daemonDlaaReset: '載入關卡或變更圖形設定會強制設為 r.TemporalAASamples=1 並停用 DLAA。',
  easyAntiCheatBlocked: '被 Easy Anti-Cheat 攔截。',
  echoDlaaAutoExposure:
    '第一關後 DLAA 將停止運作。啟用 Auto Exposure 時光源會出現頻閃；停用 Auto Exposure 時反鋸齒品質會明顯下降。',
  dx11BootFailure: '無法在 DirectX 11 模式下啟動。',
  rainCodeAaHighMaxResolution: '在遊戲設定中選擇：AA 品質 「High」 並將解析度滑桿拉至最大。',
  roboquestTaaQuality3: '在遊戲設定中選擇 TAA，並將品質設為 「3」。',
  aaUltra: '在遊戲設定中選擇 AA 「Ultra」。',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'zh-Hant', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
