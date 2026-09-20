// Generated from renderpilot-libraries by scripts/sync-renodx-i18n.mjs. Do not edit manually.

import { defineLocalizedCatalog } from '../../contract';
import type { RenoDxSourceCatalog } from './contract.generated';

export const renodxOverrides = defineLocalizedCatalog<'ru', RenoDxSourceCatalog>()({
  'renodx.black_myth_wukong.hdr':
    'В Engine.ini добавьте этот параметр для расширенного HDR-пути Unreal Engine.',
  'renodx.external.discord': 'Получите аддон из Discord, затем установите загруженный файл.',
  'renodx.external.nexus': 'Скачайте аддон с Nexus Mods, затем установите загруженный файл.',
  'renodx.generic.ue_extended': 'Использует общий профиль Unreal Engine Extended.',
  'renodx.generic.unity': 'Использует общий профиль движка Unity.',
  'renodx.generic.unreal_legacy':
    'Использует отдельный устаревший профиль Unreal Engine для этой игры.',
  'renodx.main.assassin.s.creed.iv.black.flagtm.1':
    'Мод играбелен, но некоторые графические проблемы остаются нерешёнными.',
  'renodx.main.atelier.yumia.the.alchemist.of.memories.the.envisioned.land.1':
    'Мод считается пригодным для игры, но требует более тщательного игрового тестирования.',
  'renodx.main.atlas.fallen.dx12.1': 'Установите ReShade в каталог игры Atlas Fallen\\bin.',
  'renodx.main.atlas.fallen.dx12.2':
    'FSR 2 может приводить к вылетам при использовании этого мода.',
  'renodx.main.clive.barker.s.jericho.1': 'В этой сборке вывод ACES сейчас работает некорректно.',
  'renodx.main.crimson.desert.1':
    'Частые обновления игры могут нарушать работу мода; проверяйте совместимость после каждого обновления.',
  'renodx.main.days.gone.1':
    'Эта специализированная сборка работает, но HDR-изображение всё ещё может требовать доработки.',
  'renodx.main.dead.island.riptide.definitive.edition.1':
    'Текущая специализированная сборка мода не работает.',
  'renodx.main.dragon.s.dogma.2.1':
    'После установки мода удалите shader.cache2 из каталога игры, чтобы избежать подёргиваний из-за компиляции шейдеров.',
  'renodx.main.forza.horizon.6.1':
    'Нативный HDR игры уже работает хорошо; этот мод предлагает альтернативный вариант отображения, а не исправляет HDR.',
  'renodx.main.ninja.gaiden.4.1': 'Для этого мода требуется NinjaGaidenMCFix от Lyall.',
  'renodx.main.ninja.gaiden.sigma.1': 'Для этого мода требуется NinjaGaidenMCFix от Lyall.',
  'renodx.main.ninja.gaiden.sigma.2.1': 'Для этого мода требуется NinjaGaidenMCFix от Lyall.',
  'renodx.main.opus.echo.of.starsong.full.bloom.edition.1':
    'Отключите оверлей Steam. Если проблемы сохраняются, отключите и другие оверлеи.',
  'renodx.main.raidou.remastered.the.mystery.of.the.soulless.army.1':
    'Мод требует дополнительного тестирования; в более поздних локациях могут пропускаться шейдеры.',
  'renodx.main.s.t.a.l.k.e.r.2.heart.of.chornobyl.1':
    'Установите следующие параметры изображения в настройках игры.',
  'renodx.main.sonic.unleashed.recompiled.1':
    'Отключите размытие в движении (motion blur) перед использованием этого мода.',
  'renodx.main.sonic.unleashed.recompiled.2':
    'Текущая сборка мода не работает на видеокартах GeForce RTX 50-й серии.',
  'renodx.main.steelrising.1': 'Текущая специализированная сборка мода не работает.',
  'renodx.main.the.evil.within.2.1':
    'Мод считается пригодным для игры, но требует более тщательного игрового тестирования.',
  'renodx.main.trackmania.1':
    'HDR работает хорошо, но некоторые меню кастомизации отображаются с ошибками.',
  'renodx.main.yakuza.kiwami.1.1':
    'Эта сборка разрабатывалась преимущественно для Yakuza 0; поддержка Yakuza Kiwami является вторичной.',
  'renodx.page.hdr.disable-double-tonemapping':
    'Если изображение выглядит блёклым, отключите Auto HDR и RTX HDR во избежание двойного тонмаппинга.',
  'renodx.the_first_berserker_khazan.tonemapping':
    'Проблемы с тонмаппингом сохраняются; тестирование было ограниченным.',
  'renodx.ue_extended.hdr_engine_ini': 'Для HDR-пути UE5 добавьте эти настройки в Engine.ini.',
  'renodx.ue_extended.lut_update':
    'Для UE5.3 и новее добавьте этот параметр, чтобы включить регулировку ползунков в реальном времени.',
  'renodx.ue_extended.native_hdr': 'Используйте нативный HDR.',
  'renodx.ue_extended.ue4_engine_ini_warning':
    'Общие настройки HDR в Engine.ini не рекомендуются для игр на Unreal Engine 4.',
  'renodx.ue.extended.a.way.out.1':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.abiotic.factor.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.abzu.1':
    'Выберите режим Resource Upgrade для B8G8R8A8_TYPELESS в соответствии с масштабом разрешения рендеринга игры.',
  'renodx.ue.extended.assetto.corsa.rally.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.astroneer.1': 'Ошибки цветокоррекции и мерцание всё ещё могут возникать.',
  'renodx.ue.extended.bodycam.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.borderlands.4.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.chained.together.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.chained.together.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.choo.choo.charles.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.chromatic.conundrum.1':
    'Upgrade Path несовместим с этой игрой и вызывает ошибки рендеринга.',
  'renodx.ue.extended.conan.exiles.enhanced.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.crab.champions.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.dead.as.disco.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.deep.rock.galactic.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.deep.rock.galactic.rogue.core.1':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.ue.extended.deep.rock.galactic.rogue.core.2':
    'Оставьте нативный HDR игры выключенным: нативный HDR-путь работает некорректно.',
  'renodx.ue.extended.escape.the.backrooms.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.ue.extended.escape.the.backrooms.2':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.everwind.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.far.far.west.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.frostpunk.2.1':
    'В этой игре ползунок яркости интерфейса RenoDX регулирует уровень эталонного белого (Paper White).',
  'renodx.ue.extended.grounded.2.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.hellblade.ii.senua.s.saga.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.hellblade.ii.senua.s.saga.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.hellblade.ii.senua.s.saga.3':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.hi.fi.rush.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.ue.extended.hi.fi.rush.2':
    'HDR-путь через Engine.ini приводит к пересвеченному затенению; для этой игры используется совместимый Upgrade Path.',
  'renodx.ue.extended.hi.fi.rush.3':
    'Яркие участки изображения в игре ограничены уровнем яркости интерфейса.',
  'renodx.ue.extended.inzoi.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.jusant.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.lego.batmantm.legacy.of.the.dark.knight.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.lego.batmantm.legacy.of.the.dark.knight.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.lego.batmantm.legacy.of.the.dark.knight.3':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.lies.of.p.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.lies.of.p.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.lies.of.p.3':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.little.nightmares.ii.1': 'Проблемы с цветокоррекцией сохраняются.',
  'renodx.ue.extended.mafia.the.old.country.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.meccha.chameleon.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.mortal.shell.ii.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.motor.town.behind.the.wheel.1':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.nobody.wants.to.die.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.pacific.drive.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.ue.extended.quarantine.zone.the.last.check.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.ready.or.not.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.ready.or.not.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.ue.extended.ready.or.not.3':
    'Эта конфигурация прошла лишь ограниченное игровое тестирование.',
  'renodx.ue.extended.rv.there.yet.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.s.t.a.l.k.e.r.2.heart.of.chornobyl.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.satisfactory.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.sifu.1': 'В части контента пиковая яркость остаётся ограниченной.',
  'renodx.ue.extended.solarpunk.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.star.wars.zero.companytm.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.subnautica.2.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.the.blood.of.dawnwalker.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.the.enjenir.the.engineering.physics.building.simulator.1':
    'Добавьте эти настройки в Engine.ini для расширенного HDR-пути Unreal Engine.',
  'renodx.ue.extended.the.idolm.ster.starlit.season.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.ue.extended.the.idolm.ster.starlit.season.2':
    'Освещение в некоторых сценах общения и на сцене остаётся ограниченным по яркости или не создаёт HDR-хайлайтов.',
  'renodx.ue.extended.tokyo.xtreme.racer.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.until.dawn.1': 'Включите нативный HDR в игре.',
  'renodx.ue.extended.what.remains.of.edith.finch.1':
    'Если HDR обрезается при высоком масштабировании DPI, временно установите масштаб Windows на 100%, запустите игру один раз, затем верните прежний масштаб.',
  'renodx.unity.60.parsecs.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.7.days.to.die.1':
    'Включите Swapchain Proxy при использовании динамического разрешения или масштабирования рендера.',
  'renodx.unity.advanced_restart':
    'Переключите RenoDX с Simple на Advanced, затем перезапустите игру, чтобы разблокировать все ползунки.',
  'renodx.unity.aeterna.noctis.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Output Ratio или Any Size.',
  'renodx.unity.afterparty.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.agatha.christie.death.on.the.nile.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.agent.a.a.puzzle.in.disguise.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.akiba.s.trip.hellbound.debriefed.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.all.we.need.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.altheia.the.wrath.of.aferi.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.american.fugitive.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.american.fugitive.2':
    'Установите качество графики в игре на «Среднее» (Medium) или выше.',
  'renodx.unity.amerzone.the.explorer.s.legacy.2025.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.anodyne.2.return.to.dust.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.aragami.1': 'Тонмаппинг и цветокоррекция обновляются после перезапуска уровня.',
  'renodx.unity.aragami.2.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.arkham.horror.mother.s.embrace.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.assault.android.cactus.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.atlyss.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.atomic.owl.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.autonauts.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.autonauts.vs.piratebots.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.ball.x.pit.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.batbarian.testament.of.the.primordials.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.battlestar.galactica.deadlock.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.before.your.eyes.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.beholder.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.beholder.2.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.ben.10.power.trip.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.bendy.and.the.ink.machine.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.bendy.and.the.ink.machine.2':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.bendy.secrets.of.the.machine.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.besiege.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.beyond.galaxyland.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.bionic.bay.1': 'С этим профилем игра пока не полностью готова к прохождению.',
  'renodx.unity.biped.1':
    'Загрузите сохранение, чтобы обновить тонмаппинг и цветокоррекцию после изменения ползунков.',
  'renodx.unity.blazblue.entropy.effect.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.bloody.hell.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.bo.path.of.the.teal.lotus.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.bounty.star.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.breachway.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.bubsy.4d.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.cakey.s.twisted.bakery.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.cat.quest.iii.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.children.of.morta.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.children.of.the.sun.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.circuit.superstars.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.cloudpunk.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.cloverpit.1': 'Запустите игру в режиме DirectX 11.',
  'renodx.unity.copycat.1': 'Включите Resource Upgrade для формата R10G10B10A2_TYPELESS.',
  'renodx.unity.coridden.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.creature.kitchen.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unity.ctrl.alt.ego.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.cuphead.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.dave.the.diver.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.deadcore.redux.demo.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.deadcore.redux.demo.2':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.deep.rock.survivor.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.digimon.world.next.order.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.dino.topia.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.dracomaton.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.dragon.quest.builders.1':
    'Включите свечение (bloom) в настройках игры; при желании его можно отключить позже в меню RenoDX.',
  'renodx.unity.dread.templar.1':
    'Применение или сохранение настроек RenoDX приводит к вылету игры, но настройки сохраняются.',
  'renodx.unity.dreamfall.chapters.the.final.cut.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.dreamfall.chapters.the.final.cut.2':
    'Предпросмотр сохранений отображается с ошибками.',
  'renodx.unity.drova.forsaken.kin.1':
    'Используйте режим «В окне без рамки» (Borderless Windowed).',
  'renodx.unity.drova.forsaken.kin.2': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.dungeons.2.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.dungeons.2.2':
    'Главное меню чёрное; изображение нормализуется после загрузки в игру.',
  'renodx.unity.dungeons.3.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.dungeons.4.1':
    'Измените регулятор яркости или гаммы в игре, чтобы обновить LUT после настройки RenoDX.',
  'renodx.unity.dystopika.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.eastern.exorcist.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.eastshade.1':
    'Откройте настройку яркости в игре и сохраните её, чтобы обновить LUT.',
  'renodx.unity.egging.on.1': 'Включите Resource Upgrade для формата R10G10B10A2_TYPELESS.',
  'renodx.unity.eiyuden.chronicle.hundred.heroes.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.eiyuden.chronicle.rising.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.el.paso.elsewhere.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.encased.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.end.transmission.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.enigma.of.fear.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.enter.the.gungeon.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.epistory.typing.chronicles.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.ereban.shadow.legacy.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.exit.the.gungeon.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.fabledom.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.fabledom.2':
    'Если в игре включено сглаживание, установите Compatibility Blit Copy на Scaling only.',
  'renodx.unity.fallen.aces.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.fantasy.general.ii.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.far.lone.sails.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.fe.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.firewatch.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.five.nights.at.freddy.s.into.the.pit.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.flashback.2.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.for.the.king.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.for.the.king.ii.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.forgotten.23.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.front.mission.2.remake.1':
    'Включите Resource Upgrade для формата R10G10B10A2_TYPELESS.',
  'renodx.unity.gatekeeper.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.ghost.of.a.tale.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.ghost.of.a.tale.2':
    'Включите свечение (bloom) в настройках игры; при желании его можно отключить позже в меню RenoDX.',
  'renodx.unity.giggleland.terry.s.vegetable.patch.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.great.god.grove.1':
    'Для R11G11B10_FLOAT установите Resource Upgrade в Output Ratio.',
  'renodx.unity.halve.demo.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.have.a.nice.death.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.heart.of.the.machine.1': 'Установите параметр Compatibility Scaling Offset на +1.',
  'renodx.unity.herdling.1': 'Отключите нативный HDR игры в файле конфигурации.',
  'renodx.unity.hollow.cocoon.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.hollow.cocoon.2': 'Установите качество рендеринга в игре на 100%.',
  'renodx.unity.hordelord.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.house.flipper.2.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.house.flipper.2.2':
    'Измените настройку насыщенности в игре, чтобы обновить тонмаппинг и цветокоррекцию.',
  'renodx.unity.human.fall.flat.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.hunter.hunter.nen.impact.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.icey.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.in.sound.mind.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.incision.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.jotunnslayer.hordes.of.hel.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.jotunnslayer.hordes.of.hel.2':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.katamari.damacy.reroll.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.kaze.and.the.wild.masks.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unity.keep.talking.and.nobody.explodes.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.keywe.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.kill.knight.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.killer.frequency.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.kona.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.lego.voyagers.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.little.kitty.big.city.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.little.noah.scion.of.paradise.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.little.witch.in.the.woods.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.lost.in.random.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.lost.in.vivo.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.lumines.arise.1': 'Для R11G11B10_FLOAT установите Resource Upgrade в Output Ratio.',
  'renodx.unity.lysfanga.the.time.shift.warrior.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.mages.of.mystralia.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.mai.child.of.ages.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.maid.of.sker.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.metal.hellsinger.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.monument.valley.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.monument.valley.2.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.mouthwashing.1': 'Используйте режим экрана Borderless.',
  'renodx.unity.mouthwashing.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.my.friend.pedro.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.my.friend.pedro.2':
    'Перезапустите уровень, чтобы обновить тонмаппинг и цветокоррекцию.',
  'renodx.unity.my.friendly.neighborhood.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.necropolis.brutal.edition.1':
    'Загружайте ReShade через Ultimate ASI Loader для этой 32-битной игры.',
  'renodx.unity.neon.abyss.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.neoverse.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.new.super.lucky.s.tale.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.nocturnal.1': 'Включите Resource Upgrade для формата R10G10B10A2_TYPELESS.',
  'renodx.unity.node.the.last.favor.of.the.antarii.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.nottolot.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.observation.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.oddworld.soulstorm.enhanced.edition.1':
    'Включите Resource Upgrade для формата R10G10B10A2_TYPELESS.',
  'renodx.unity.oddworld.soulstorm.enhanced.edition.2':
    'Начните заново с контрольной точки или перезапустите уровень, чтобы обновить тонмаппинг и цветокоррекцию.',
  'renodx.unity.operation.wolf.returns.first.mission.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.order.13.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.order.13.2':
    'Измените настройку яркости в игре, чтобы обновить тонмаппинг и цветокоррекцию.',
  'renodx.unity.outer.wilds.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.overcooked.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.overcooked.2.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.overcooked.all.you.can.eat.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.oxenfree.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.oxenfree.ii.lost.signals.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.party.club.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.peak.1': 'Запустите игру с параметром DirectX 11.',
  'renodx.unity.pillars.of.eternity.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.pillars.of.eternity.ii.deadfire.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.playing.kafka.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.praey.for.the.gods.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.prince.of.persia.the.lost.crown.1':
    'В этой игре RenoDX не может влиять на цветокоррекцию помимо параметров Peak и UI Brightness.',
  'renodx.unity.prince.of.persia.the.lost.crown.2':
    'Используйте специальный мод Luma Framework для полной коррекции.',
  'renodx.unity.quern.undying.thoughts.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.r.e.p.o.1': 'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unity.reka.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.reset_display_controls':
    'Оставляйте настройки яркости, контрастности и гаммы в игре по умолчанию, если в описании игры не указано иное.',
  'renodx.unity.reveil.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.road.96.mile.0.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.road.redemption.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.ruined.king.a.league.of.legends.storytm.1':
    'Фон меню паузы отображается с ошибками.',
  'renodx.unity.sektori.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.shadow.labyrinth.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.shadow.of.the.road.demo.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.shadows.awakening.1':
    'Изменения тонмаппинга и цветокоррекции отображаются не в реальном времени.',
  'renodx.unity.shadows.of.doubt.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.shape.of.dreams.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.shift.87.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.skate.story.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.skate.story.2': 'Отключите нативный HDR в игре.',
  'renodx.unity.sker.ritual.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.sker.ritual.2':
    'Откройте настройки графики игры и нажмите «Применить», чтобы обновить тонмаппинг и цветокоррекцию.',
  'renodx.unity.songs.of.silence.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.soulstone.survivors.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.space.crew.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.spacebase.startopia.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.spiritfall.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.spooky.s.jump.scare.mansion.hd.renovation.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.stick.fight.the.game.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.streets.of.rogue.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.summoner.s.war.chronicles.1': 'Включите свечение (bloom) в настройках игры.',
  'renodx.unity.super.crazy.rhythm.castle.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.superhot.1': 'Установите параметр Swapchain Encoding на Gamma.',
  'renodx.unity.superhot.mind.control.delete.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.sworn.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.syberia.remastered.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.syberia.the.world.before.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.tainted.grail.conquest.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.tales.of.berseria.remastered.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.tales.of.berseria.remastered.2':
    'Коррекция также затрагивает анимационные видеовставки (FMV).',
  'renodx.unity.tales.of.the.shire.a.the.lord.of.the.ringstm.game.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.tales.of.xillia.remastered.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.teenage.mutant.ninja.turtles.mutants.unleashed.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.terratech.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.the.first.tree.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.the.karters.2.turbo.charged.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.the.knightling.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.the.pedestrian.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.the.rogue.prince.of.persia.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.the.upturned.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.tin.can.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.tinykin.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.touhou.dystopian.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.towa.and.the.guardians.of.the.sacred.tree.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.towaga.among.shadows.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.towaga.among.shadows.2': 'Включите постобработку в настройках игры.',
  'renodx.unity.trailmakers.1': 'Включите постобработку на любом уровне качества.',
  'renodx.unity.trailmakers.2': 'Включите опцию Internal HDR в настройках игры.',
  'renodx.unity.turbo.golf.racing.1': 'Включите постобработку в настройках графики игры.',
  'renodx.unity.two.point.hospital.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.ultros.1': 'Включите цветокоррекцию в настройках игры.',
  'renodx.unity.undying.1':
    'При использовании FSR 1 установите Resource Upgrade для R11G11B10_FLOAT как минимум в Output Ratio.',
  'renodx.unity.unruly.heroes.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.v.rising.1':
    'В одиночной игре откройте и закройте меню паузы или настроек, чтобы обновить LUT.',
  'renodx.unity.v.rising.2':
    'В сетевой игре встаньте на солнце или откройте Графика > Калибровка яркости и подтвердите значение по умолчанию 50, чтобы обновить LUT.',
  'renodx.unity.valley.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.viewfinder.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.viewfinder.2': 'Включите постобработку в настройках игры.',
  'renodx.unity.void.crew.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.void.crew.2': 'DLSS ограничивает цветовое пространство до BT.709.',
  'renodx.unity.wasteland.2.directors.cut.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.wasteland.3.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.wavetale.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.we.were.here.expeditions.the.friendship.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.windblown.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unity.windowed':
    'Избегайте эксклюзивного полноэкранного режима; используйте режим «В окне без рамки» или оконный.',
  'renodx.unity.witchspring.r.the.story.of.pieberry.1':
    'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unity.wizard.with.a.gun.1': 'Включите Swapchain Proxy в RenoDX.',
  'renodx.unity.wizard.with.a.gun.2':
    'Существенно увеличьте ползунок Highlights, так как яркие участки по умолчанию тусклые.',
  'renodx.unity.yooka.replaylee.1': 'Включите Resource Upgrade для формата R11G11B10_FLOAT.',
  'renodx.unreal_legacy.advanced_restart':
    'Переключите RenoDX с Simple на Advanced, затем перезапустите игру, чтобы разблокировать все ползунки.',
  'renodx.unreal_legacy.slider_refresh':
    'Во многих играх на Unreal изменения ползунков применяются только после смены сцены или возврата из меню.',
  'renodx.unreal.age.of.darkness.final.stand.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Output Ratio.',
  'renodx.unreal.aliens.dark.descent.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unreal.aphelion.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.ashen.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.asterigos.curse.of.the.stars.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.atomic.heart.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.banishers.ghosts.of.new.eden.1':
    'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.bloodstained.ritual.of.the.night.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.borderlands.3.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.bramble.the.mountain.king.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.bright.memory.infinite.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.bright.memory.infinite.2': 'Используйте DirectX 12.',
  'renodx.unreal.call.of.the.sea.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.cepheus.protocol.1':
    'Используйте этот Resource Upgrade, если масштаб разрешения или процент экрана отличается от 100%.',
  'renodx.unreal.chernobylite.1': 'Используйте DirectX 12.',
  'renodx.unreal.chernobylite.2':
    'Изменение качества DLSS может временно отключить чрезмерную резкость в игре, но это действие нужно повторять в каждой сессии.',
  'renodx.unreal.chernobylite.3': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.chorus.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.clash.artifacts.of.chaos.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.colony.ship.a.post.earth.role.playing.game.1':
    'Некоторые меню и экран загрузки всё ещё требуют исправлений.',
  'renodx.unreal.crisis.core.ff7.reunion.1':
    'RenoDX автоматически переводит B8G8R8A8_TYPELESS в режим Output Size.',
  'renodx.unreal.daemon.x.machina.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.dahlia.view.1':
    'Отключите некорректно работающий нативный HDR игры, особенно при использовании DirectX 12.',
  'renodx.unreal.dahlia.view.2': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.darksiders.3.1': 'Полоски здоровья обычных врагов обновляются некорректно.',
  'renodx.unreal.darksiders.genesis.1':
    'Некоторый текст на экране загрузки может отображаться сплошными блоками.',
  'renodx.unreal.darksiders.genesis.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.daymare.1994.sandcastle.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.deadlink.1': 'Используйте DirectX 12.',
  'renodx.unreal.deadlink.2': 'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unreal.demon.slayer.kny.hinokami.chronicles.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.dnf.duel.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.dragon.ball.z.kakarot.1': 'Некоторые цвета остаются неточными.',
  'renodx.unreal.dragon.ball.z.kakarot.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.dragon.quest.iii.hd.2d.remake.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.echo.point.nova.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.en.garde.1': 'DirectX 12 приводит к вылетам; используйте DirectX 11.',
  'renodx.unreal.enotria.the.last.song.1':
    'Изменения ползунков применяются только после смены сцены или после возврата из главного меню.',
  'renodx.unreal.escape.from.ever.after.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.everspace.1':
    'Используйте 64-битную версию RSG-Win64-Shipping.exe. Эта конфигурация тестировалась только с версией GOG.',
  'renodx.unreal.everspace.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.everspace.2.1': 'Используйте DirectX 12.',
  'renodx.unreal.everspace.2.2': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.evil.west.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.flyknight.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.flyknight.filter_warning': 'Внутриигровые фильтры могут изменять отображение HDR.',
  'renodx.unreal.flyknight.limited_testing':
    'Эта конфигурация была протестирована лишь ограниченно.',
  'renodx.unreal.forgive.me.father.1': 'Некоторый текст может отображаться сплошными блоками.',
  'renodx.unreal.forgive.me.father.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.fort.solis.1': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.ghostrunner-2.limited_testing':
    'Проблемы с тонмаппингом сохраняются; тестирование было ограниченным.',
  'renodx.unreal.ghostrunner.2.1': 'Вывод остаётся ограниченным цветовым пространством BT.709.',
  'renodx.unreal.ghostrunner.2.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.gothic.1.remake.1':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.granblue.fantasy.versus.1':
    'Яркие участки персонажей и карты ограничены уровнем яркости интерфейса.',
  'renodx.unreal.granblue.fantasy.versus.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.grounded.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.guilty.gear.strive.1':
    'Полоски здоровья не обновляются, на экране выбора персонажа могут быть графические артефакты, а шкала напряжения слишком яркая.',
  'renodx.unreal.guilty.gear.strive.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.hell.is.us.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.hell.is.us.2': 'Этот профиль тестировался только с демоверсией.',
  'renodx.unreal.hell.is.us.3': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.hogwarts.legacy.1':
    'Используйте сборку, опубликованную в канале RenoDX Unreal Engine.',
  'renodx.unreal.hyper.light.breaker.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.indika.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.industria.1': 'Используйте DirectX 12.',
  'renodx.unreal.industria.2': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.infinity.nikki.1':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.islands.of.insight.1':
    'Вывод остаётся ограниченным цветовым пространством BT.709.',
  'renodx.unreal.islands.of.insight.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.jdm.japanese.drift.master.1':
    'Для применения изменений ползунков требуется смена сцены.',
  'renodx.unreal.jdm.japanese.drift.master.2':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.journey.to.the.savage.planet.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.just.die.already.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.kena.bridge.of.spirits.1':
    'Яркость в кат-сценах ограничена уровнем яркости интерфейса.',
  'renodx.unreal.kingdom.hearts.iii.1': 'Может наблюдаться мерцание текстур.',
  'renodx.unreal.kingdom.hearts.iii.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.kitten.burst.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.lay.of.the.land.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.lego.2k.drive.1':
    'Мод работает без Resource Upgrades, хотя свет фар некоторых автомобилей может оставаться ограниченным по яркости.',
  'renodx.unreal.life.is.strange.true.colors.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.lightyear.frontier.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Ratio.',
  'renodx.unreal.little.nightmares.iii.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.lords.of.the.fallen.2023.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.lords.of.the.fallen.2023.2':
    'Отключите Easy Anti-Cheat перед использованием мода.',
  'renodx.unreal.lords.of.the.fallen.2023.3':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.lost.soul.aside.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.lost.soul.aside.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.marvel.s.midnight.suns.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.marvel.s.midnight.suns.2':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.master.detective.archives.rain.code.plus.1':
    'RenoDX автоматически переводит B8G8R8A8_TYPELESS в режим Output Ratio.',
  'renodx.unreal.metal.eden.1':
    'В главном меню и на экране возрождения могут возникать графические артефакты.',
  'renodx.unreal.metal.eden.2': 'Этот профиль тестировался только с демоверсией.',
  'renodx.unreal.metal.eden.3': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.metro.gravity.1': 'Используйте пресет цветокоррекции SDR Grading Bypass.',
  'renodx.unreal.metro.gravity.2': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Any Size.',
  'renodx.unreal.miasma.chronicles.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.motorslice.1': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.murky.divers.1':
    'Может потребоваться ограничение цветового охвата (Gamut clamping).',
  'renodx.unreal.murky.divers.2': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Any Size.',
  'renodx.unreal.necromunda.hired.gun.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.neverness.to.everness.1':
    'Следуйте подробным инструкциям по настройке RenoDX для этой игры.',
  'renodx.unreal.ninja.gaiden.2.black.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.ninja.gaiden.2.black.2':
    'В этой игре изменения ползунков RenoDX применяются не в реальном времени.',
  'renodx.unreal.octopath.traveler.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.octopath.traveler.ii.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.outriders.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.palworld.1':
    'Отключите DLSS, так как он ограничивает вывод цветовым диапазоном SDR.',
  'renodx.unreal.palworld.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.palworld.limited_testing':
    'Эта конфигурация была протестирована лишь ограниченно.',
  'renodx.unreal.police.simulator.patrol.officer.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.police.simulator.patrol.officer.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.psychonauts.2.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.reanimal.1':
    'В игре уже есть полноценная собственная реализация HDR; RenoDX обычно не требуется.',
  'renodx.unreal.redout.2.1': 'Игра может аварийно завершаться с этим профилем.',
  'renodx.unreal.redout.2.2': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.remnant.2.1': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.returnal.1': 'Отключайте нативный HDR в игре после каждого запуска.',
  'renodx.unreal.returnal.2': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.runescape.dragonwilds.1':
    'Если игра работает нестабильно, переключитесь на DirectX 11 в настройках графики.',
  'renodx.unreal.runescape.dragonwilds.2':
    'Изменения ползунков применяются только после смены сцены или после возврата из главного меню.',
  'renodx.unreal.sackboy.a.big.adventure.1': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.scarlet.nexus.1': 'Сцены в стиле визуальной новеллы отображаются с ошибками.',
  'renodx.unreal.scarlet.nexus.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.severed.steel.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.six.days.in.fallujah.1':
    'Вокруг солнца может появляться тёмный артефакт, а дым может отображаться слишком тёмным.',
  'renodx.unreal.six.days.in.fallujah.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.solar.ash.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.sonic.racing.crossworlds.1':
    'Изменения ползунков вступают в силу после перезапуска трассы.',
  'renodx.unreal.sonic.racing.crossworlds.2':
    'Яркие элементы ограничены яркостью интерфейса, а небо в Water Palace остаётся чрезмерно ярким.',
  'renodx.unreal.sonic.racing.crossworlds.3':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.soulstice.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.south.of.midnight.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Ratio.',
  'renodx.unreal.split.fiction.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.split.fiction.2':
    'Для применения изменений ползунков может потребоваться смена сцены.',
  'renodx.unreal.spongebob.squarepants.battle.for.bikini.bottom.rehydrated.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.squirrel.with.a.gun.1': 'В RenoDX установите параметр Swap Chain Format на scRGB.',
  'renodx.unreal.stellar.blade.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.stellar.blade.2':
    'На ультрашироких мониторах используйте соотношение сторон «Авто», чтобы избежать проблем в мини-игре NIKKE.',
  'renodx.unreal.styx.shards.of.darkness.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.sword.and.fairy.7.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.system.shock.remake.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.tales.of.arise.1': 'Свечение (bloom) иногда может отображаться некорректно.',
  'renodx.unreal.tales.of.arise.2':
    'Режимы SMAA и SMAA + TAA не поддерживаются; используйте TAA или отключите сглаживание.',
  'renodx.unreal.tales.of.kenzera.zau.1':
    'Обновите libxess.dll перед использованием XeSS, чтобы избежать вылетов.',
  'renodx.unreal.tales.of.kenzera.zau.2':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.tekken.7.1': 'На некоторых уровнях яркость ограничена уровнем яркости интерфейса.',
  'renodx.unreal.tekken.7.2': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.terminator.resistance.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.ascent.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.awesome.adventures.of.captain.spirit.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.cabin.factory.1':
    'Мод работает без Resource Upgrades, однако цветокоррекция не исправляется.',
  'renodx.unreal.the.forgotten.city.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.invincible.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.medium.1': 'Отключите нативный HDR в игре.',
  'renodx.unreal.the.medium.2': 'Яркие участки в мире духов ограничены уровнем яркости интерфейса.',
  'renodx.unreal.the.medium.3': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Any Size.',
  'renodx.unreal.the.midnight.walk.1':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.occupation.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.outer.worlds.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unreal.the.outer.worlds.spacer.s.choice.edition.1':
    'Этот опциональный апгрейд предотвращает отключение HDR в некоторых сценах, но приводит к ошибкам воспроизведения FMV.',
  'renodx.unreal.the.outlast.trials.1':
    'Для R8G8B8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.the.smurfs.dreams.1':
    'Ползунок яркости интерфейса в RenoDX также влияет на видеоролики (FMV).',
  'renodx.unreal.the.smurfs.dreams.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.the.thaumaturge.1': 'В некоторых сценах яркость остаётся ограниченной.',
  'renodx.unreal.the.thaumaturge.2':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.threshold.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Any Size.',
  'renodx.unreal.thymesia.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.tiny.tina.s.wonderlands.1': 'Отключите FidelityFX Sharpening в игре.',
  'renodx.unreal.tiny.tina.s.wonderlands.2': 'Примените следующие настройки RenoDX для этой игры.',
  'renodx.unreal.trek.to.yomi.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.vampire.the.masquerade.bloodlines.2.1':
    'Яркость в кат-сценах остаётся ограниченной.',
  'renodx.unreal.vampire.the.masquerade.bloodlines.2.2':
    'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
  'renodx.unreal.vampyr.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.warhammer.40.000.boltgun.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.we.happy.few.1':
    'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.weird.west.1': 'Для B8G8R8A8_TYPELESS установите Resource Upgrade в Output Size.',
  'renodx.unreal.witchfire.1': 'Для R10G10B10A2_UNORM установите Resource Upgrade в Output Size.',
});
