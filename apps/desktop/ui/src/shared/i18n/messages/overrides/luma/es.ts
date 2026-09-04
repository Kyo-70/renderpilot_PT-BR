import { defineLocalizedCatalog } from '../../contract';
import {
  expandLumaTranslations,
  type LumaMessageTranslations,
  type LumaSourceCatalog,
} from './schema';

const translations = {
  publicMatchmaking:
    'Evita el emparejamiento público oficial mientras Luma esté instalado. Podría ocasionar un bloqueo.',
  deusExBorisEnb:
    'Incompatible con Boris ENB (DX9). Funciona con la versión Director’s Cut y la original. El mod Gold Filter Restoration es redundante con Luma.',
  dlssOnlyNoHdr: 'Este perfil solo admite DLSS; HDR no es compatible actualmente.',
  hatsuneExclusiveFullscreen:
    'Si surgen problemas de visualización, evita la pantalla completa exclusiva. Pulsa Alt+Enter para cambiar de modo.',
  heavyRainSteamUltrawide:
    'La compatibilidad con pantalla ultraancha puede funcionar solo en Steam.',
  xboxStore: 'No es compatible con la versión de Xbox Store.',
  metroWindowed:
    'Requiere modo ventana o sin bordes, configurado mediante mods o desactivando la pantalla completa en la configuración del juego.',
  metroBorderless: 'Usa el modo ventana sin bordes.',
  preyData: 'Mantén los archivos de datos adicionales de Luma para Prey junto con el complemento.',
  massEffectNativeAa:
    'Solo están disponibles los modos DLAA / FSR 3 Native AA; esto no es superresolución DLSS ni FSR.',
  manualLaunchArgument: 'Añade este argumento de inicio manualmente.',
  aceFxaaHigh: 'En los ajustes del juego, usa: AA «FXAA High».',
  manualEngineIni: 'Aplica los siguientes ajustes manualmente en Engine.ini.',
  callSeaEpic: 'En los ajustes del juego, selecciona calidad general «Epic».',
  codeVeinAaHighest: 'En los ajustes del juego, selecciona AA Highest.',
  crabHighAntialiasing: 'En los ajustes del juego, selecciona High Anti-aliasing Type.',
  crashAaMedium: 'En los ajustes del juego, usa: calidad de suavizado al menos Medium (2x).',
  clashAaVeryHigh: 'En los ajustes del juego, selecciona calidad de AA «Very High».',
  closeToSunAa4x: 'En los ajustes del juego, selecciona AA 4X.',
  darksidersAaEpic: 'En los ajustes del juego, selecciona AA Epic.',
  daymareOptiscalerUuu:
    'Luma funciona por sí solo, pero se bloquea si se combina con OptiScaler o UUU.',
  deadlineUltra: 'En los ajustes del juego, selecciona «Ultra».',
  dieYoungTaa: 'En los ajustes del juego, selecciona TAA «High» o «Epic».',
  dnfCharacterSelection: 'El suavizado no funciona en la pantalla de selección de personajes.',
  kakarotBdzKfix:
    'En los ajustes del juego, usa: TAA. Usa BDZKFix para la versión Legacy o su fork actualizado para la versión HD.',
  filamentAaHigh: 'En los ajustes del juego, selecciona AA «High» o «Very High».',
  goatHighAa: 'En los ajustes del juego, selecciona High AA.',
  guiltyGearStriveAa:
    'El suavizado no funciona en la pantalla de selección de personajes. En el juego: AA «Temporal Anti Aliasing». En Engine.ini en [SystemSettings], añade: r.DefaultFeature.AntiAliasing=2 y r.PostProcessAAQuality=4.',
  itTakesTwoTitle: 'Solo funciona durante la secuencia de la pantalla de título.',
  aaHigh: 'En los ajustes del juego, selecciona AA «High».',
  kh3Txaa: 'En los ajustes del juego, usa: «TXAA».',
  mutantMotionBlur:
    'En los ajustes del juego, usa: AA «High». Para mayor claridad de movimiento, se recomienda r.motionblur.amount=0 en Engine.ini.',
  orcsAaHigh: 'En los ajustes del juego, selecciona calidad de AA «High».',
  projectWingmanFxaa: 'En los ajustes del juego, usa: AA «FXAA».',
  scarletNexusTxaa: 'En los ajustes del juego, selecciona AA «TXAA».',
  scornOptiscaler:
    'Tiene compatibilidad nativa con FSR 2.1; se puede añadir DLSS u otros escaladores mediante OptiScaler.',
  smtLyallFix: 'Requiere Lyall’s Fix para forzar TAA.',
  spiritNorthUltra: 'En los ajustes del juego, selecciona calidad gráfica «Ultra».',
  spyroHighTaa: 'En los ajustes del juego, selecciona High TAA.',
  supralandTaa: 'En los ajustes del juego, selecciona AA «Temporal Anti Aliasing».',
  talesAriseSdk: 'Requiere Arise-SDK con UseUE4TAA=true.',
  tekkenNoD3D9Ex: 'Requiere el argumento de inicio -nod3d9ex.',
  tetrisFxaa6: 'En los ajustes del juego, usa: AA «FXAA:6» y escala de renderizado al 100%.',
  sinkingCityOriginal:
    'Compatible con la versión original; la compatibilidad con la edición Remastered no está confirmada.',
  vampyrTxaa6x: 'En los ajustes del juego, selecciona AA TXAA 6X.',
  edithFinchExit:
    'DLAA funciona sin cambios adicionales, pero es posible que el juego no se cierre por completo al salir. OptiScaler puede solucionar este problema.',
  edithFinch4k:
    'El juego es inestable en resolución 4K. Ajusta Effects en Low antes de aplicar los cambios manuales en Engine.ini.',
  sherlockDx11Performance:
    'El argumento de inicio -dx11 reduce el rendimiento de la CPU. Al usar DLAA con Auto Exposure activado, pueden aparecer bordes dentados en la hierba.',
  fallout4DlssGtaoOnly: 'Actualmente este perfil solo admite DLSS y GTAO.',
  biomutantAaHighOrMax: 'En los ajustes del juego, selecciona AA «High» o «Max».',
  blairWitchTxaaFull: 'En los ajustes del juego, usa: TXAA y escala de resolución «Full».',
  flickeringIssues: 'Pueden producirse parpadeos visuales.',
  brambleEpicVram:
    'La calidad Epic puede llenar progresivamente la VRAM y provocar tirones. Evita alternar repetidamente entre High y Epic con Luma activo.',
  daemonDlaaReset:
    'Cargar un nivel o cambiar los ajustes gráficos fuerza r.TemporalAASamples=1 y desactiva DLAA.',
  easyAntiCheatBlocked: 'Bloqueado por Easy Anti-Cheat.',
  echoDlaaAutoExposure:
    'DLAA deja de funcionar tras el primer nivel. Con Auto Exposure activado, las fuentes de luz parpadean; con Auto Exposure desactivado, la calidad del suavizado disminuye notablemente.',
  dx11BootFailure: 'No se ejecuta en modo DirectX 11.',
  rainCodeAaHighMaxResolution:
    'En los ajustes del juego, usa: calidad de AA «High» y escala de resolución al máximo.',
  roboquestTaaQuality3: 'En los ajustes del juego, selecciona TAA y calidad «3».',
  aaUltra: 'En los ajustes del juego, selecciona AA «Ultra».',
} as const satisfies LumaMessageTranslations;

export const lumaOverrides = defineLocalizedCatalog<'es', LumaSourceCatalog>()(
  expandLumaTranslations(translations),
);
