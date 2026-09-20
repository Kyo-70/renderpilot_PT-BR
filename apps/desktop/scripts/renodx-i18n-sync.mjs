import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

import {
  analyzeMessageTemplate,
  formatGeneratedSource,
  validateRenodxContract,
} from './i18n-contracts.mjs';

export const RENODX_LOCALES = Object.freeze([
  'de',
  'es',
  'fr',
  'ja',
  'pt-BR',
  'ru',
  'zh-Hans',
  'zh-Hant',
]);

const APP_ROOT = path.resolve(import.meta.dirname, '..');
const OVERRIDES_ROOT = path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/renodx');
const CONTRACT_PATH = path.join(OVERRIDES_ROOT, 'source.generated.json');
const GENERATED_HEADER =
  '// Generated from renderpilot-libraries by scripts/sync-renodx-i18n.mjs. Do not edit manually.\n';

function fail(message) {
  throw new Error(`RenoDX i18n sync failed: ${message}`);
}

function nonEmptyString(value, context) {
  if (typeof value !== 'string' || value.trim() === '') {
    fail(`${context} must be a non-empty string`);
  }
  return value;
}

export function projectRenodxI18nSource(producer) {
  if (producer?.schema_version !== 1 || !Array.isArray(producer.messages)) {
    fail('producer message contract has an unsupported shape');
  }

  const seenIds = new Set();
  const localized = Object.fromEntries(RENODX_LOCALES.map((locale) => [locale, {}]));
  const messages = producer.messages.map((message, index) => {
    const id = nonEmptyString(message?.id, `messages[${index}].id`);
    if (seenIds.has(id)) {
      fail(`duplicate message ID ${id}`);
    }
    seenIds.add(id);

    const translations = message?.translations;
    if (!translations || typeof translations !== 'object' || Array.isArray(translations)) {
      fail(`message ${id} translations have an unsupported shape`);
    }
    const localeKeys = Object.keys(translations).toSorted();
    if (JSON.stringify(localeKeys) !== JSON.stringify(RENODX_LOCALES.toSorted())) {
      fail(`message ${id} does not have exact locale coverage`);
    }
    for (const locale of RENODX_LOCALES) {
      const translation = nonEmptyString(
        translations[locale],
        `message ${id} ${locale} translation`,
      );
      const analysis = analyzeMessageTemplate(translation);
      if (!analysis.valid || analysis.placeholders.length > 0) {
        fail(`message ${id} ${locale} translation must not contain placeholders`);
      }
      localized[locale][id] = translation;
    }

    return {
      id,
      sourceText: nonEmptyString(message?.fallback_text, `message ${id} fallback_text`),
      kind: nonEmptyString(message?.kind, `message ${id} kind`),
      context: nonEmptyString(message?.context, `message ${id} context`),
    };
  });

  const contract = { schemaVersion: 1, messages };
  try {
    validateRenodxContract(contract);
  } catch (cause) {
    fail(
      cause instanceof Error
        ? cause.message.replace(/^i18n contract generation failed:\s*/u, '')
        : String(cause),
    );
  }

  return {
    contract,
    localized,
  };
}

function renderLocalizedCatalog(locale, messages) {
  return `${GENERATED_HEADER}
import { defineLocalizedCatalog } from '../../contract';
import type { RenoDxSourceCatalog } from './contract.generated';

export const renodxOverrides = defineLocalizedCatalog<'${locale}', RenoDxSourceCatalog>()(${JSON.stringify(messages, null, 2)});
`;
}

export async function createRenodxI18nOutputs(producer) {
  const { contract, localized } = projectRenodxI18nSource(producer);
  const outputs = new Map([[CONTRACT_PATH, `${JSON.stringify(contract, null, 2)}\n`]]);

  for (const locale of RENODX_LOCALES) {
    const filePath = path.join(OVERRIDES_ROOT, `${locale}.ts`);
    outputs.set(
      filePath,
      await formatGeneratedSource(filePath, renderLocalizedCatalog(locale, localized[locale])),
    );
  }
  return outputs;
}

export async function checkRenodxI18nOutputs(outputs) {
  const stale = [];
  for (const [filePath, expected] of outputs) {
    let actual = null;
    try {
      actual = await readFile(filePath, 'utf8');
    } catch (error) {
      if (error?.code !== 'ENOENT') {
        throw error;
      }
    }
    if (actual !== expected) {
      stale.push(path.relative(APP_ROOT, filePath).replaceAll(path.sep, '/'));
    }
  }
  return stale;
}

export async function writeRenodxI18nOutputs(outputs) {
  for (const [filePath, source] of outputs) {
    await mkdir(path.dirname(filePath), { recursive: true });
    await writeFile(filePath, source, 'utf8');
  }
}
