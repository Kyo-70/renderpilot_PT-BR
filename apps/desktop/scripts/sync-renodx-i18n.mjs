import { readFile } from 'node:fs/promises';
import path from 'node:path';

import {
  checkRenodxI18nOutputs,
  createRenodxI18nOutputs,
  writeRenodxI18nOutputs,
} from './renodx-i18n-sync.mjs';

const [mode, producerFlag, producerRootValue] = process.argv.slice(2);
if (
  (mode !== '--check' && mode !== '--write') ||
  producerFlag !== '--producer-root' ||
  !producerRootValue
) {
  throw new Error(
    'Usage: node scripts/sync-renodx-i18n.mjs --check|--write --producer-root <path>',
  );
}

const producerRoot = path.resolve(producerRootValue);
const producer = JSON.parse(
  await readFile(path.join(producerRoot, 'catalogs/addons/renodx/messages.json'), 'utf8'),
);
const outputs = await createRenodxI18nOutputs(producer);

if (mode === '--write') {
  await writeRenodxI18nOutputs(outputs);
  console.log(`Generated ${outputs.size} RenoDX i18n snapshot files.`);
} else {
  const stale = await checkRenodxI18nOutputs(outputs);
  if (stale.length > 0) {
    console.error('Generated RenoDX i18n snapshots are stale:');
    for (const file of stale) {
      console.error(`- ${file}`);
    }
    process.exitCode = 1;
  } else {
    console.log(`Verified ${outputs.size} generated RenoDX i18n snapshot files.`);
  }
}
