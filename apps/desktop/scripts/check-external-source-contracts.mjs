import { readFile } from 'node:fs/promises';
import path from 'node:path';

import { parseExternalSourceCheckArguments } from './external-source-check/arguments.mjs';
import {
  verifyLumaSourceContract,
  verifyNvapiSourceContract,
  verifyOptiscalerSourceContract,
  verifyRenodxSourceContract,
} from './external-source-contracts.mjs';
import { checkLumaI18nOutputs, createLumaI18nOutputs } from './luma-i18n-sync.mjs';
import {
  checkOptiscalerI18nOutputs,
  createOptiscalerI18nOutputs,
} from './optiscaler-i18n-sync.mjs';
import { checkRenodxI18nOutputs, createRenodxI18nOutputs } from './renodx-i18n-sync.mjs';

const APP_ROOT = path.resolve(import.meta.dirname, '..');
const REPOSITORY_ROOT = path.resolve(APP_ROOT, '..', '..');

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, 'utf8'));
}

const { producerRoot } = parseExternalSourceCheckArguments(process.argv.slice(2));
const [
  lumaContract,
  producerLuma,
  bundledNvapi,
  producerNvapi,
  optiscalerContract,
  producerOptiscaler,
  renodxContract,
  producerRenodx,
] = await Promise.all([
  readJson(path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/luma/source.generated.json')),
  readJson(path.join(producerRoot, 'catalogs/addons/luma/messages.json')),
  readJson(
    path.join(
      REPOSITORY_ROOT,
      'crates/renderpilot-orchestration/src/dlss/bundled/dlss_settings.json',
    ),
  ),
  readJson(path.join(producerRoot, 'dlss_settings.json')),
  readJson(
    path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/optiscaler/source.generated.json'),
  ),
  readJson(path.join(producerRoot, 'catalogs/addons/optiscaler/compatibility/messages.json')),
  readJson(
    path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/renodx/source.generated.json'),
  ),
  readJson(path.join(producerRoot, 'catalogs/addons/renodx/messages.json')),
]);

const luma = verifyLumaSourceContract(lumaContract, producerLuma);
const nvapi = verifyNvapiSourceContract(bundledNvapi, producerNvapi);
const optiscaler = verifyOptiscalerSourceContract(optiscalerContract, producerOptiscaler);
const renodx = verifyRenodxSourceContract(renodxContract, producerRenodx);

const lumaOutputs = await createLumaI18nOutputs(producerLuma);
const staleLumaOutputs = await checkLumaI18nOutputs(lumaOutputs);
if (staleLumaOutputs.length > 0) {
  throw new Error(
    `external i18n source check failed: generated Luma i18n snapshots are stale: ${staleLumaOutputs.join(', ')}`,
  );
}

const optiscalerOutputs = await createOptiscalerI18nOutputs(producerOptiscaler);
const staleOptiscalerOutputs = await checkOptiscalerI18nOutputs(optiscalerOutputs);
if (staleOptiscalerOutputs.length > 0) {
  throw new Error(
    `external i18n source check failed: generated OptiScaler i18n snapshots are stale: ${staleOptiscalerOutputs.join(', ')}`,
  );
}

const renodxOutputs = await createRenodxI18nOutputs(producerRenodx);
const staleRenodxOutputs = await checkRenodxI18nOutputs(renodxOutputs);
if (staleRenodxOutputs.length > 0) {
  throw new Error(
    `external i18n source check failed: generated RenoDX i18n snapshots are stale: ${staleRenodxOutputs.join(', ')}`,
  );
}

console.log(
  `Verified external i18n sources: Luma ${luma.messageCount} messages; NVAPI ${nvapi.settingCount} settings / ${nvapi.messageCount} messages; OptiScaler ${optiscaler.messageCount} messages; RenoDX ${renodx.messageCount} messages.`,
);
