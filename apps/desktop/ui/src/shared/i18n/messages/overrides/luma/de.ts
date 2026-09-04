import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking:
    'Vermeide offizielles öffentliches Matchmaking, solange Luma installiert ist. Dies kann zu einer Sperre führen.',
  deusExBorisEnb:
    'Nicht kompatibel mit Boris ENB (DX9). Kompatibel mit Director’s Cut und der Originalausgabe. Die Mod Gold Filter Restoration ist bei Verwendung von Luma überflüssig.',
  dlssOnlyNoHdr: 'Dieses Profil unterstützt nur DLSS; HDR wird derzeit nicht unterstützt.',
  hatsuneExclusiveFullscreen:
    'Vermeide bei Anzeigeproblemen den exklusiven Vollbildmodus. Drücke Alt+Enter, um den Modus zu wechseln.',
  heavyRainSteamUltrawide: 'Ultrawide-Unterstützung funktioniert möglicherweise nur auf Steam.',
  xboxStore: 'Nicht kompatibel mit der Xbox-Store-Version.',
  metroWindowed:
    'Erfordert Fenster- oder rahmenlosen Modus, konfiguriert über Mods oder durch Deaktivieren von Vollbild in der Spielkonfiguration.',
  metroBorderless: 'Verwende den rahmenlosen Fenstermodus.',
  preyData: 'Bewahre die zusätzlichen Luma-Datendateien für Prey zusammen mit dem Add-on auf.',
  massEffectNativeAa:
    'Nur DLAA / FSR 3 Native AA sind verfügbar; dies ist kein DLSS- oder FSR-Super-Resolution-Modus.',
  manualLaunchArgument: 'Füge dieses Startargument manuell hinzu.',
  aceFxaaHigh: 'Wähle in den Spieleinstellungen: AA „FXAA High“.',
  manualEngineIni: 'Übernimm die folgenden Einstellungen manuell in Engine.ini.',
  callSeaEpic: 'Wähle in den Spieleinstellungen die Gesamtqualität „Epic“ aus.',
  codeVeinAaHighest: 'Wähle in den Spieleinstellungen AA Highest aus.',
  crabHighAntialiasing: 'Wähle in den Spieleinstellungen High Anti-aliasing Type aus.',
  crashAaMedium:
    'Wähle in den Spieleinstellungen mindestens die Antialiasing-Qualität Medium (2x).',
  clashAaVeryHigh: 'Wähle in den Spieleinstellungen die AA-Qualität „Very High“ aus.',
  closeToSunAa4x: 'Wähle in den Spieleinstellungen AA 4X aus.',
  darksidersAaEpic: 'Wähle in den Spieleinstellungen AA Epic aus.',
  daymareOptiscalerUuu:
    'Luma funktioniert allein, stürzt jedoch in Kombination mit OptiScaler oder UUU ab.',
  deadlineUltra: 'Wähle in den Spieleinstellungen „Ultra“ aus.',
  dieYoungTaa: 'Wähle in den Spieleinstellungen TAA „High“ oder „Epic“ aus.',
  dnfCharacterSelection: 'Antialiasing funktioniert im Charakterauswahlbildschirm nicht.',
  kakarotBdzKfix:
    'Wähle in den Spieleinstellungen: TAA. Nutze BDZKFix für die Legacy-Version oder dessen aktualisierten Fork für die HD-Version.',
  filamentAaHigh: 'Wähle in den Spieleinstellungen AA „High“ oder „Very High“ aus.',
  goatHighAa: 'Wähle in den Spieleinstellungen High AA aus.',
  guiltyGearStriveAa:
    'Antialiasing funktioniert im Charakterauswahlbildschirm nicht. Wähle im Spiel: AA „Temporal Anti Aliasing“. Ergänze in der Engine.ini unter [SystemSettings]: r.DefaultFeature.AntiAliasing=2 und r.PostProcessAAQuality=4.',
  itTakesTwoTitle: 'Funktioniert nur während der Titelsequenz.',
  aaHigh: 'Wähle in den Spieleinstellungen AA „High“ aus.',
  kh3Txaa: 'Wähle in den Spieleinstellungen: „TXAA“.',
  mutantMotionBlur:
    'Wähle in den Spieleinstellungen: AA „High“. Für klarere Bewegung setze r.motionblur.amount=0 in der Engine.ini.',
  orcsAaHigh: 'Wähle in den Spieleinstellungen die AA-Qualität „High“ aus.',
  projectWingmanFxaa: 'Wähle in den Spieleinstellungen: AA „FXAA“.',
  scarletNexusTxaa: 'Wähle in den Spieleinstellungen AA „TXAA“ aus.',
  scornOptiscaler:
    'Das Spiel unterstützt FSR 2.1 nativ; DLSS oder andere Upscaler können über OptiScaler hinzugefügt werden.',
  smtLyallFix: 'Erfordert Lyall’s Fix, um TAA zu erzwingen.',
  spiritNorthUltra: 'Wähle in den Spieleinstellungen die Grafikqualität „Ultra“ aus.',
  spyroHighTaa: 'Wähle in den Spieleinstellungen High TAA aus.',
  supralandTaa: 'Wähle in den Spieleinstellungen AA „Temporal Anti Aliasing“ aus.',
  talesAriseSdk: 'Erfordert Arise-SDK mit UseUE4TAA=true.',
  tekkenNoD3D9Ex: 'Erfordert das Startargument -nod3d9ex.',
  tetrisFxaa6: 'Wähle in den Spieleinstellungen: AA „FXAA:6“ und 100 % Render-Skalierung.',
  sinkingCityOriginal:
    'Kompatibel mit der Originalversion; Kompatibilität mit der Remastered-Version ist unbestätigt.',
  vampyrTxaa6x: 'Wähle in den Spieleinstellungen AA TXAA 6X aus.',
  edithFinchExit:
    'DLAA funktioniert ohne weitere Änderungen, doch das Spiel schließt sich nach dem Beenden möglicherweise nicht vollständig. OptiScaler kann das Problem beheben.',
  edithFinch4k:
    'Das Spiel läuft bei 4K-Auflösung instabil. Stelle Effects auf Low, bevor du manuelle Engine.ini-Einstellungen anwendest.',
  sherlockDx11Performance:
    'Das Startargument -dx11 führt zu schlechter CPU-Leistung. Bei DLAA mit aktiviertem Auto Exposure entstehen gezackte Kanten an Gras.',
  fallout4DlssGtaoOnly: 'Dieses Profil unterstützt derzeit nur DLSS und GTAO.',
  biomutantAaHighOrMax: 'Wähle in den Spieleinstellungen AA „High“ oder „Max“.',
  blairWitchTxaaFull: 'Wähle in den Spieleinstellungen: TXAA und Auflösungsskalierung „Full“.',
  flickeringIssues: 'Visuelles Flackern kann auftreten.',
  brambleEpicVram:
    'Die Qualitätsstufe Epic kann den VRAM stetig füllen und Ruckeln verursachen. Wechsle bei aktivem Luma nicht wiederholt zwischen High und Epic.',
  daemonDlaaReset:
    'Das Laden eines Levels oder das Ändern der Grafikeinstellungen erzwingt r.TemporalAASamples=1 und deaktiviert DLAA.',
  easyAntiCheatBlocked: 'Durch Easy Anti-Cheat blockiert.',
  echoDlaaAutoExposure:
    'DLAA funktioniert nach dem ersten Level nicht mehr. Bei aktiviertem Auto Exposure flackern Lichtquellen; bei deaktiviertem Auto Exposure verschlechtert sich die Antialiasing-Qualität deutlich.',
  dx11BootFailure: 'Startet nicht im DirectX-11-Modus.',
  rainCodeAaHighMaxResolution:
    'Wähle in den Spieleinstellungen: AA-Qualität „High“ und maximale Auflösungsskalierung.',
  roboquestTaaQuality3: 'Wähle in den Spieleinstellungen TAA und Qualität „3“.',
  aaUltra: 'Wähle in den Spieleinstellungen AA „Ultra“ aus.',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'de', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
