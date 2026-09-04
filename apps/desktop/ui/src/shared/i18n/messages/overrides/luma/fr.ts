import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking:
    'Évitez le matchmaking public officiel lorsque Luma est installé. Cela pourrait entraîner un bannissement.',
  deusExBorisEnb:
    'Incompatible avec Boris ENB (DX9). Fonctionne avec la version Director’s Cut et l’édition originale. Le mod Gold Filter Restoration est superflu avec Luma.',
  dlssOnlyNoHdr:
    'Ce profil prend en charge uniquement le DLSS ; le HDR n’est pas pris en charge actuellement.',
  hatsuneExclusiveFullscreen:
    'En cas de problème d’affichage, évitez le plein écran exclusif. Appuyez sur Alt+Entrée pour changer de mode.',
  heavyRainSteamUltrawide:
    'La prise en charge du format ultra-large peut ne fonctionner que sur Steam.',
  xboxStore: 'Incompatible avec la version Xbox Store.',
  metroWindowed:
    'Nécessite le mode fenêtré ou sans bordures, configuré via des mods ou en désactivant le plein écran dans la configuration du jeu.',
  metroBorderless: 'Utilisez le mode fenêtré sans bordures.',
  preyData:
    'Conservez les fichiers de données Luma supplémentaires pour Prey avec le module complémentaire.',
  massEffectNativeAa:
    'Seuls les modes DLAA / FSR 3 Native AA sont disponibles ; il ne s’agit pas de super-résolution DLSS ou FSR.',
  manualLaunchArgument: 'Ajoutez cet argument de lancement manuellement.',
  aceFxaaHigh: 'Dans les paramètres du jeu, utilisez : AA « FXAA High ».',
  manualEngineIni: 'Appliquez manuellement les paramètres suivants dans Engine.ini.',
  callSeaEpic: 'Dans les paramètres du jeu, sélectionnez la qualité globale « Epic ».',
  codeVeinAaHighest: 'Dans les paramètres du jeu, sélectionnez AA Highest.',
  crabHighAntialiasing: 'Dans les paramètres du jeu, sélectionnez High Anti-aliasing Type.',
  crashAaMedium:
    'Dans les paramètres du jeu, utilisez : qualité d’anticrénelage au moins Medium (2x).',
  clashAaVeryHigh: 'Dans les paramètres du jeu, sélectionnez la qualité d’AA « Very High ».',
  closeToSunAa4x: 'Dans les paramètres du jeu, sélectionnez AA 4X.',
  darksidersAaEpic: 'Dans les paramètres du jeu, sélectionnez AA Epic.',
  daymareOptiscalerUuu:
    'Luma fonctionne seul, mais plante lorsqu’il est combiné avec OptiScaler ou UUU.',
  deadlineUltra: 'Dans les paramètres du jeu, sélectionnez « Ultra ».',
  dieYoungTaa: 'Dans les paramètres du jeu, sélectionnez TAA « High » ou « Epic ».',
  dnfCharacterSelection:
    'L’anticrénelage ne fonctionne pas sur l’écran de sélection des personnages.',
  kakarotBdzKfix:
    'Dans les paramètres du jeu, utilisez : TAA. Utilisez BDZKFix pour la version Legacy ou son fork mis à jour pour la version HD.',
  filamentAaHigh: 'Dans les paramètres du jeu, sélectionnez AA « High » ou « Very High ».',
  goatHighAa: 'Dans les paramètres du jeu, sélectionnez High AA.',
  guiltyGearStriveAa:
    'L’anticrénelage ne fonctionne pas sur l’écran de sélection des personnages. En jeu : AA « Temporal Anti Aliasing ». Dans Engine.ini sous [SystemSettings], ajoutez : r.DefaultFeature.AntiAliasing=2 et r.PostProcessAAQuality=4.',
  itTakesTwoTitle: 'Fonctionne uniquement pendant la séquence de l’écran titre.',
  aaHigh: 'Dans les paramètres du jeu, sélectionnez AA « High ».',
  kh3Txaa: 'Dans les paramètres du jeu, utilisez : « TXAA ».',
  mutantMotionBlur:
    'Dans les paramètres du jeu, utilisez : AA « High ». Pour une meilleure netteté des mouvements, définissez r.motionblur.amount=0 dans Engine.ini.',
  orcsAaHigh: 'Dans les paramètres du jeu, sélectionnez la qualité d’AA « High ».',
  projectWingmanFxaa: 'Dans les paramètres du jeu, utilisez : AA « FXAA ».',
  scarletNexusTxaa: 'Dans les paramètres du jeu, sélectionnez AA « TXAA ».',
  scornOptiscaler:
    'Prend en charge FSR 2.1 nativement ; DLSS ou d’autres upscalers peuvent être ajoutés via OptiScaler.',
  smtLyallFix: 'Nécessite Lyall’s Fix pour forcer le TAA.',
  spiritNorthUltra: 'Dans les paramètres du jeu, sélectionnez la qualité graphique « Ultra ».',
  spyroHighTaa: 'Dans les paramètres du jeu, sélectionnez High TAA.',
  supralandTaa: 'Dans les paramètres du jeu, sélectionnez AA « Temporal Anti Aliasing ».',
  talesAriseSdk: 'Nécessite Arise-SDK avec UseUE4TAA=true.',
  tekkenNoD3D9Ex: 'Nécessite l’argument de lancement -nod3d9ex.',
  tetrisFxaa6: 'Dans les paramètres du jeu, utilisez : AA « FXAA:6 » et échelle de rendu à 100 %.',
  sinkingCityOriginal:
    'Compatible avec la version originale ; compatibilité non confirmée avec l’édition Remastered.',
  vampyrTxaa6x: 'Dans les paramètres du jeu, sélectionnez AA TXAA 6X.',
  edithFinchExit:
    'DLAA fonctionne sans modifications supplémentaires, mais le jeu peut ne pas se fermer complètement après l’avoir quitté. OptiScaler peut résoudre ce problème.',
  edithFinch4k:
    'Le jeu est instable en résolution 4K. Réglez Effects sur Low avant d’appliquer les paramètres manuels dans Engine.ini.',
  sherlockDx11Performance:
    'L’argument de lancement -dx11 entraîne de mauvaises performances CPU. Avec DLAA et Auto Exposure activé, des bordures crénelées apparaissent sur l’herbe.',
  fallout4DlssGtaoOnly: 'Ce profil ne prend actuellement en charge que DLSS et GTAO.',
  biomutantAaHighOrMax: 'Dans les paramètres du jeu, sélectionnez AA « High » ou « Max ».',
  blairWitchTxaaFull:
    'Dans les paramètres du jeu, utilisez : TXAA et échelle de résolution « Full ».',
  flickeringIssues: 'Des clignotements visuels peuvent survenir.',
  brambleEpicVram:
    'La qualité Epic peut saturer progressivement la VRAM et provoquer des saccades. Évitez de basculer à répétition entre High et Epic lorsque Luma est actif.',
  daemonDlaaReset:
    'Le chargement d’un niveau ou la modification des réglages graphiques force r.TemporalAASamples=1 et désactive DLAA.',
  easyAntiCheatBlocked: 'Bloqué par Easy Anti-Cheat.',
  echoDlaaAutoExposure:
    'DLAA cesse de fonctionner après le premier niveau. Avec Auto Exposure activé, les sources lumineuses clignotent ; avec Auto Exposure désactivé, la qualité d’anticrénelage se dégrade nettement.',
  dx11BootFailure: 'Ne se lance pas en mode DirectX 11.',
  rainCodeAaHighMaxResolution:
    'Dans les paramètres du jeu, utilisez : qualité d’AA « High » et curseur de résolution au maximum.',
  roboquestTaaQuality3: 'Dans les paramètres du jeu, sélectionnez TAA et la qualité « 3 ».',
  aaUltra: 'Dans les paramètres du jeu, sélectionnez AA « Ultra ».',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'fr', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
