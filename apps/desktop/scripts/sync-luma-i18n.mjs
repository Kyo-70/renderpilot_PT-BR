import { readFile } from 'node:fs/promises';
import path from 'node:path';

import {
  checkLumaI18nOutputs,
  createLumaI18nOutputs,
  writeLumaI18nOutputs,
} from './luma-i18n-sync.mjs';

const [mode, producerFlag, producerRootValue] = process.argv.slice(2);
if (
  (mode !== '--check' && mode !== '--write') ||
  producerFlag !== '--producer-root' ||
  !producerRootValue
) {
  throw new Error('Usage: node scripts/sync-luma-i18n.mjs --check|--write --producer-root <path>');
}

const producerRoot = path.resolve(producerRootValue);
const producer = JSON.parse(
  await readFile(path.join(producerRoot, 'catalogs/addons/luma/messages.json'), 'utf8'),
);
const outputs = await createLumaI18nOutputs(producer);

if (mode === '--write') {
  await writeLumaI18nOutputs(outputs);
  console.log(`Generated ${outputs.size} Luma i18n snapshot files.`);
} else {
  const stale = await checkLumaI18nOutputs(outputs);
  if (stale.length > 0) {
    console.error('Generated Luma i18n snapshots are stale:');
    for (const file of stale) {
      console.error(`- ${file}`);
    }
    process.exitCode = 1;
  } else {
    console.log(`Verified ${outputs.size} generated Luma i18n snapshot files.`);
  }
}
