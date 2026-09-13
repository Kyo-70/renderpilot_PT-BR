import { readFile } from 'node:fs/promises';
import path from 'node:path';

import {
  checkOptiscalerI18nOutputs,
  createOptiscalerI18nOutputs,
  writeOptiscalerI18nOutputs,
} from './optiscaler-i18n-sync.mjs';

const [mode, producerFlag, producerRootValue] = process.argv.slice(2);
if (
  (mode !== '--check' && mode !== '--write') ||
  producerFlag !== '--producer-root' ||
  !producerRootValue
) {
  throw new Error(
    'Usage: node scripts/sync-optiscaler-i18n.mjs --check|--write --producer-root <path>',
  );
}

const producerRoot = path.resolve(producerRootValue);
const producer = JSON.parse(
  await readFile(
    path.join(producerRoot, 'catalogs/addons/optiscaler/compatibility/messages.json'),
    'utf8',
  ),
);
const outputs = await createOptiscalerI18nOutputs(producer);

if (mode === '--write') {
  await writeOptiscalerI18nOutputs(outputs);
  console.log(`Generated ${outputs.size} OptiScaler i18n snapshot files.`);
} else {
  const stale = await checkOptiscalerI18nOutputs(outputs);
  if (stale.length > 0) {
    console.error('Generated OptiScaler i18n snapshots are stale:');
    for (const file of stale) {
      console.error(`- ${file}`);
    }
    process.exitCode = 1;
  } else {
    console.log(`Verified ${outputs.size} generated OptiScaler i18n snapshot files.`);
  }
}
