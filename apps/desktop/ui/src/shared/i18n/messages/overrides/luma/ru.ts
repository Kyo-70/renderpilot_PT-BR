import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking:
    'Не используйте официальный публичный матчмейкинг, пока Luma установлена. Это может привести к бану.',
  deusExBorisEnb:
    'Несовместимо с Boris ENB (DX9). Совместимо с Director’s Cut и оригинальным изданием. Мод Gold Filter Restoration при использовании Luma не требуется.',
  dlssOnlyNoHdr: 'Этот профиль добавляет только поддержку DLSS; HDR сейчас не поддерживается.',
  hatsuneExclusiveFullscreen:
    'При проблемах с отображением избегайте эксклюзивного полноэкранного режима. Чтобы переключить режим, нажмите Alt+Enter.',
  heavyRainSteamUltrawide: 'Ультраширокий режим может работать только при запуске через Steam.',
  xboxStore: 'Несовместимо с версией из Xbox Store.',
  metroWindowed:
    'Требуется оконный или безрамочный режим: через моды либо отключением полноэкранного режима в конфиге игры.',
  metroBorderless: 'Используйте безрамочный оконный режим.',
  preyData: 'Дополнительные файлы данных Luma для Prey должны оставаться рядом с аддоном.',
  massEffectNativeAa:
    'Доступны только режимы сглаживания DLAA / FSR 3 Native AA; суперразрешение DLSS или FSR не поддерживается.',
  manualLaunchArgument: 'Добавьте этот аргумент запуска вручную.',
  aceFxaaHigh: 'В настройках игры выберите AA «FXAA High».',
  manualEngineIni: 'Вручную добавьте в Engine.ini следующие настройки.',
  callSeaEpic: 'В настройках игры выберите общее качество «Epic».',
  codeVeinAaHighest: 'В настройках игры выберите AA Highest.',
  crabHighAntialiasing: 'В настройках игры выберите High Anti-aliasing Type.',
  crashAaMedium: 'В настройках игры выберите качество сглаживания не ниже Medium (2x).',
  clashAaVeryHigh: 'В настройках игры выберите качество AA «Very High».',
  closeToSunAa4x: 'В настройках игры выберите AA 4X.',
  darksidersAaEpic: 'В настройках игры выберите AA Epic.',
  daymareOptiscalerUuu:
    'Luma работает автономно, но вылетает при совместном использовании с OptiScaler или UUU.',
  deadlineUltra: 'В настройках игры выберите «Ultra».',
  dieYoungTaa: 'В настройках игры выберите TAA «High» или «Epic».',
  dnfCharacterSelection: 'Сглаживание не работает на экране выбора персонажа.',
  kakarotBdzKfix:
    'В настройках игры выберите TAA. Используйте BDZKFix для Legacy-версии либо его обновлённый форк для HD-версии.',
  filamentAaHigh: 'В настройках игры выберите AA «High» или «Very High».',
  goatHighAa: 'В настройках игры выберите High AA.',
  guiltyGearStriveAa:
    'Сглаживание не работает на экране выбора персонажа. В игре: AA «Temporal Anti Aliasing». В Engine.ini в секции [SystemSettings] добавьте: r.DefaultFeature.AntiAliasing=2 и r.PostProcessAAQuality=4.',
  itTakesTwoTitle: 'Работает только во время вступительной заставки на титульном экране.',
  aaHigh: 'В настройках игры выберите AA «High».',
  kh3Txaa: 'В настройках игры выберите «TXAA».',
  mutantMotionBlur:
    'В настройках игры выберите AA «High». Для более чёткого движения рекомендуется задать r.motionblur.amount=0 в Engine.ini.',
  orcsAaHigh: 'В настройках игры выберите качество AA «High».',
  projectWingmanFxaa: 'В настройках игры выберите AA «FXAA».',
  scarletNexusTxaa: 'В настройках игры выберите AA «TXAA».',
  scornOptiscaler:
    'В игре есть нативная поддержка FSR 2.1; DLSS или другие апскейлеры можно добавить через OptiScaler.',
  smtLyallFix: 'Для принудительного включения TAA требуется Lyall’s Fix.',
  spiritNorthUltra: 'В настройках игры выберите качество графики «Ultra».',
  spyroHighTaa: 'В настройках игры выберите High TAA.',
  supralandTaa: 'В настройках игры выберите AA «Temporal Anti Aliasing».',
  talesAriseSdk: 'Требуется Arise-SDK с параметром UseUE4TAA=true.',
  tekkenNoD3D9Ex: 'Требуется аргумент запуска -nod3d9ex.',
  tetrisFxaa6: 'В настройках игры выберите AA «FXAA:6» и масштаб рендеринга 100%.',
  sinkingCityOriginal:
    'Оригинальная версия работает. Совместимость с изданием Remastered не подтверждена.',
  vampyrTxaa6x: 'В настройках игры выберите AA TXAA 6X.',
  edithFinchExit:
    'DLAA работает без дополнительных изменений, но игра может не завершаться полностью после выхода. OptiScaler может устранить эту проблему.',
  edithFinch4k:
    'Игра работает нестабильно в разрешении 4K. Перед внесением настроек в Engine.ini установите Effects на Low.',
  sherlockDx11Performance:
    'Аргумент запуска -dx11 снижает производительность CPU. При включённом Auto Exposure в режиме DLAA на траве появляются зубчатые края.',
  fallout4DlssGtaoOnly: 'Сейчас этот профиль поддерживает только DLSS и GTAO.',
  biomutantAaHighOrMax: 'В настройках игры выберите AA «High» или «Max».',
  blairWitchTxaaFull: 'В настройках игры выберите TXAA и масштаб разрешения «Full».',
  flickeringIssues: 'Возможны проблемы с мерцанием изображения.',
  brambleEpicVram:
    'Качество Epic может со временем заполнить видеопамять (VRAM) и вызвать микрофризы. Избегайте многократного переключения между High и Epic при активной Luma.',
  daemonDlaaReset:
    'Загрузка уровня или изменение настроек графики принудительно задаёт r.TemporalAASamples=1 и отключает DLAA.',
  easyAntiCheatBlocked: 'Заблокировано системой Easy Anti-Cheat.',
  echoDlaaAutoExposure:
    'После первого уровня DLAA перестаёт работать. При включённом Auto Exposure источники света мерцают, а при отключённом качество сглаживания заметно ухудшается.',
  dx11BootFailure: 'Не запускается в режиме DirectX 11.',
  rainCodeAaHighMaxResolution:
    'В настройках игры выберите качество AA «High» и установите ползунок разрешения на максимум.',
  roboquestTaaQuality3: 'В настройках игры выберите TAA и качество «3».',
  aaUltra: 'В настройках игры выберите AA «Ultra».',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'ru', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
