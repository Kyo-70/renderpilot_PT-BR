import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking:
    'Luma のインストール中は公式の公開マッチメイキングを利用しないでください。BAN される可能性があります。',
  deusExBorisEnb:
    'Boris ENB（DX9）とは互換性がありません。Director’s Cut およびオリジナル版と互換性があります。Luma 使用時は Gold Filter Restoration MOD は不要です。',
  dlssOnlyNoHdr: 'このプロファイルは DLSS のみをサポートしています（現在 HDR には非対応）。',
  hatsuneExclusiveFullscreen:
    '表示に問題が生じる場合は排他的フルスクリーンを避けてください。Alt+Enter で表示モードを切り替えられます。',
  heavyRainSteamUltrawide: 'ウルトラワイド対応は Steam でのみ動作する可能性があります。',
  xboxStore: 'Xbox Store 版とは互換性がありません。',
  metroWindowed:
    'ウィンドウまたはボーダーレスモードが必要です。MOD を使用するか、ゲーム設定ファイルでフルスクリーンを無効化してください。',
  metroBorderless: 'ボーダーレスウィンドウモードを使用してください。',
  preyData: 'Prey 用の追加 Luma データファイルは、アドオンと同じ場所に置いてください。',
  massEffectNativeAa:
    '利用できるのは DLAA / FSR 3 Native AA モードのみで、DLSS または FSR の超解像ではありません。',
  manualLaunchArgument: 'この起動引数を手動で追加してください。',
  aceFxaaHigh: 'ゲーム設定: AA「FXAA High」を選択してください。',
  manualEngineIni: '次の設定を Engine.ini に手動で適用してください。',
  callSeaEpic: 'ゲーム設定で全体品質を「Epic」にしてください。',
  codeVeinAaHighest: 'ゲーム設定で AA を Highest にしてください。',
  crabHighAntialiasing: 'ゲーム設定で High Anti-aliasing Type を選択してください。',
  crashAaMedium: 'ゲーム設定: アンチエイリアス品質を少なくとも Medium (2x) に設定してください。',
  clashAaVeryHigh: 'ゲーム設定で AA 品質を「Very High」にしてください。',
  closeToSunAa4x: 'ゲーム設定で AA を 4X にしてください。',
  darksidersAaEpic: 'ゲーム設定で AA を Epic にしてください。',
  daymareOptiscalerUuu:
    'Luma は単体では動作しますが、OptiScaler または UUU と併用するとクラッシュします。',
  deadlineUltra: 'ゲーム設定で「Ultra」を選択してください。',
  dieYoungTaa: 'ゲーム設定で TAA を「High」または「Epic」にしてください。',
  dnfCharacterSelection: 'キャラクター選択画面ではアンチエイリアスが機能しません。',
  kakarotBdzKfix:
    'ゲーム設定: TAA を選択してください。Legacy 版では BDZKFix、HD 版ではその更新フォークを使用してください。',
  filamentAaHigh: 'ゲーム設定で AA を「High」または「Very High」にしてください。',
  goatHighAa: 'ゲーム設定で High AA を選択してください。',
  guiltyGearStriveAa:
    'キャラクター選択画面ではアンチエイリアスが機能しません。ゲーム内設定: AA「Temporal Anti Aliasing」。Engine.ini の [SystemSettings] に追加: r.DefaultFeature.AntiAliasing=2 および r.PostProcessAAQuality=4。',
  itTakesTwoTitle: 'タイトル画面のシーケンス中のみ動作します。',
  aaHigh: 'ゲーム設定で AA を「High」にしてください。',
  kh3Txaa: 'ゲーム設定:「TXAA」を選択してください。',
  mutantMotionBlur:
    'ゲーム設定: AA「High」。動きの鮮明さを向上させるには、Engine.ini で r.motionblur.amount=0 を設定することを推奨します。',
  orcsAaHigh: 'ゲーム設定で AA 品質を「High」にしてください。',
  projectWingmanFxaa: 'ゲーム設定: AA「FXAA」を選択してください。',
  scarletNexusTxaa: 'ゲーム設定で AA を「TXAA」にしてください。',
  scornOptiscaler:
    'FSR 2.1 をネイティブサポートしています。DLSS やその他のアップスケーラーは OptiScaler 経由で追加できます。',
  smtLyallFix: 'TAA を強制するには Lyall’s Fix が必要です。',
  spiritNorthUltra: 'ゲーム設定でグラフィック品質を「Ultra」にしてください。',
  spyroHighTaa: 'ゲーム設定で High TAA を選択してください。',
  supralandTaa: 'ゲーム設定で AA を「Temporal Anti Aliasing」にしてください。',
  talesAriseSdk: 'UseUE4TAA=true を設定した Arise-SDK が必要です。',
  tekkenNoD3D9Ex: '起動引数 -nod3d9ex が必要です。',
  tetrisFxaa6: 'ゲーム設定: AA「FXAA:6」およびレンダリングスケール 100% を選択してください。',
  sinkingCityOriginal: 'オリジナル版と互換性があります。Remastered 版との互換性は未確認です。',
  vampyrTxaa6x: 'ゲーム設定で AA を TXAA 6X にしてください。',
  edithFinchExit:
    'DLAA は追加の変更なしで動作しますが、終了後にゲームが完全に閉じないことがあります。OptiScaler で解決できる場合があります。',
  edithFinch4k:
    'ゲームは 4K 解像度で動作が不安定になります。手動で Engine.ini の設定を適用する前に Effects を Low に設定してください。',
  sherlockDx11Performance:
    '起動引数 -dx11 は CPU 性能の低下を引き起こします。Auto Exposure を有効にした DLAA では草の縁にジャギーが生じます。',
  fallout4DlssGtaoOnly: '現在、このプロファイルがサポートするのは DLSS と GTAO のみです。',
  biomutantAaHighOrMax: 'ゲーム設定で AA を「High」または「Max」にしてください。',
  blairWitchTxaaFull: 'ゲーム設定: TXAA および解像度スケーラビリティ「Full」を設定してください。',
  flickeringIssues: '画面のちらつきが発生する場合があります。',
  brambleEpicVram:
    'Epic 品質は VRAM を徐々に消費し、カクつきの原因になることがあります。Luma の動作中に High と Epic を何度も切り替えないでください。',
  daemonDlaaReset:
    'レベルのロードやグラフィック設定の変更により、強制的に r.TemporalAASamples=1 が設定され DLAA が無効化されます。',
  easyAntiCheatBlocked: 'Easy Anti-Cheat によりブロックされます。',
  echoDlaaAutoExposure:
    '最初のレベルをクリアすると DLAA が機能しなくなります。Auto Exposure を有効にすると光源が点滅し、無効にするとアンチエイリアス品質が著しく低下します。',
  dx11BootFailure: 'DirectX 11 モードでは起動しません。',
  rainCodeAaHighMaxResolution:
    'ゲーム設定: AA 品質「High」および最大解像度スライダーを設定してください。',
  roboquestTaaQuality3: 'ゲーム設定で TAA と品質「3」を選択してください。',
  aaUltra: 'ゲーム設定で AA を「Ultra」にしてください。',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'ja', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
