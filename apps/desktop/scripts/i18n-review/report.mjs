import { readFile } from 'node:fs/promises';
import path from 'node:path';
import ts from 'typescript';

import {
  objectLiteralEntries,
  parseTypeScriptSource,
  stringLiteralValue,
  variableInitializer,
} from '../typescript-ast.mjs';
import {
  ExternalContractValidationError,
  projectSupportedNvapiCatalog,
  validateLumaContract,
  validateRenodxContract,
} from '../external-contract-core.mjs';
import { validateOptiscalerContract } from '../i18n-contracts/validator.mjs';

export const REVIEW_LOCALES = ['ru', 'de', 'es', 'pt-BR', 'fr', 'ja', 'zh-Hans', 'zh-Hant'];
export const REVIEW_FORMATS = ['tsv', 'json'];

const APP_ROOT = path.resolve(import.meta.dirname, '../..');

function fail(message) {
  throw new Error(`i18n review failed: ${message}`);
}

function validateExternalContract(operation) {
  try {
    return operation();
  } catch (cause) {
    if (cause instanceof ExternalContractValidationError) {
      fail(cause.message);
    }
    throw cause;
  }
}

export function parseTranslationSource(
  sourceText,
  fileName = 'translations.ts',
  { variableName = 'translations', factoryName = null } = {},
) {
  const source = parseTypeScriptSource(sourceText, fileName, fail);
  let initializer = variableInitializer(source, variableName, fail);
  if (factoryName !== null) {
    if (
      !ts.isCallExpression(initializer) ||
      initializer.arguments.length !== 1 ||
      !ts.isCallExpression(initializer.expression) ||
      !ts.isIdentifier(initializer.expression.expression) ||
      initializer.expression.expression.text !== factoryName ||
      initializer.expression.arguments.length !== 0
    ) {
      fail(`${fileName} must initialize ${variableName} with ${factoryName}<...>()({...})`);
    }
    [initializer] = initializer.arguments;
  }
  const translations = {};
  for (const [key, expression] of objectLiteralEntries(
    initializer,
    `${fileName} translations`,
    fail,
  )) {
    if (Object.hasOwn(translations, key)) {
      fail(`${fileName} contains duplicate translation ${key}`);
    }
    translations[key] = stringLiteralValue(expression, `${fileName} translation ${key}`, fail);
  }
  return translations;
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, 'utf8'));
}

function assertExactKeys(actual, expected, context) {
  const actualKeys = Object.keys(actual).toSorted();
  const expectedKeys = [...expected].toSorted();
  if (JSON.stringify(actualKeys) !== JSON.stringify(expectedKeys)) {
    fail(`${context} keys do not match the source contract`);
  }
}

function nvapiContext(key) {
  const [, setting, ...kind] = key.split('.');
  return `setting.${setting}.${kind.join('.')}`;
}

export async function createReviewReport(locale) {
  if (!REVIEW_LOCALES.includes(locale)) {
    fail(`unsupported locale ${JSON.stringify(locale)}`);
  }
  const lumaDirectory = path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/luma');
  const nvapiDirectory = path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/nvapi');
  const optiscalerDirectory = path.join(
    APP_ROOT,
    'ui/src/shared/i18n/messages/overrides/optiscaler',
  );
  const renodxDirectory = path.join(APP_ROOT, 'ui/src/shared/i18n/messages/overrides/renodx');
  const [
    lumaContract,
    nvapiCatalog,
    optiscalerContract,
    renodxContract,
    editorialPolicy,
    lumaSource,
    nvapiSource,
    optiscalerSource,
    renodxSource,
  ] = await Promise.all([
    readJson(path.join(lumaDirectory, 'source.generated.json')),
    readJson(
      path.resolve(
        APP_ROOT,
        '../../crates/renderpilot-orchestration/src/dlss/bundled/dlss_settings.json',
      ),
    ),
    readJson(path.join(optiscalerDirectory, 'source.generated.json')),
    readJson(path.join(renodxDirectory, 'source.generated.json')),
    readJson(path.join(APP_ROOT, 'data/i18n-editorial-policy.json')),
    readFile(path.join(lumaDirectory, `${locale}.ts`), 'utf8'),
    readFile(path.join(nvapiDirectory, `${locale}.ts`), 'utf8'),
    readFile(path.join(optiscalerDirectory, `${locale}.ts`), 'utf8'),
    readFile(path.join(renodxDirectory, `${locale}.ts`), 'utf8'),
  ]);

  const lumaTranslations = parseTranslationSource(lumaSource, `luma/${locale}.ts`, {
    variableName: 'lumaOverrides',
    factoryName: 'defineLocalizedCatalog',
  });
  const nvapiTranslations = parseTranslationSource(nvapiSource, `nvapi/${locale}.ts`);
  const optiscalerTranslations = parseTranslationSource(
    optiscalerSource,
    `optiscaler/${locale}.ts`,
    { variableName: 'optiscalerOverrides', factoryName: 'defineLocalizedCatalog' },
  );
  const renodxTranslations = parseTranslationSource(renodxSource, `renodx/${locale}.ts`, {
    variableName: 'renodxOverrides',
    factoryName: 'defineLocalizedCatalog',
  });
  const luma = validateExternalContract(() => validateLumaContract(lumaContract));
  const nvapi = validateExternalContract(() => projectSupportedNvapiCatalog(nvapiCatalog));
  const optiscaler = validateOptiscalerContract(optiscalerContract);
  const renodx = validateExternalContract(() => validateRenodxContract(renodxContract));
  assertExactKeys(lumaTranslations, Object.keys(luma.sourceCatalog), `Luma ${locale}`);
  assertExactKeys(
    optiscalerTranslations,
    Object.keys(optiscaler.sourceCatalog),
    `OptiScaler ${locale}`,
  );
  assertExactKeys(renodxTranslations, Object.keys(renodx.sourceCatalog), `RenoDX ${locale}`);

  const verbatim = new Set(editorialPolicy.nvapiVerbatimValues);
  const nvapiSources = [];
  const nvapiRows = Object.entries(nvapi.sourceCatalog).map(([key, source]) => {
    if (!verbatim.has(source)) {
      nvapiSources.push(source);
    }
    return {
      key,
      context: nvapiContext(key),
      source,
      translation: verbatim.has(source) ? source : nvapiTranslations[source],
    };
  });
  assertExactKeys(nvapiTranslations, new Set(nvapiSources), `NVAPI ${locale}`);

  const lumaRows = Object.entries(luma.sourceCatalog).map(([key, source]) => ({
    key,
    context: luma.contexts[key],
    source,
    translation: lumaTranslations[key],
  }));
  const optiscalerRows = Object.entries(optiscaler.sourceCatalog).map(([key, source]) => ({
    key,
    context: `guidance.${optiscaler.contexts[key]}`,
    source,
    translation: optiscalerTranslations[key],
  }));
  const renodxRows = Object.entries(renodx.sourceCatalog).map(([key, source]) => ({
    key,
    context: renodx.contexts[key],
    source,
    translation: renodxTranslations[key],
  }));
  const messages = [...lumaRows, ...nvapiRows, ...optiscalerRows, ...renodxRows];
  for (const row of messages) {
    if (typeof row.translation !== 'string' || row.translation.trim() === '') {
      fail(`${locale} has no translation for ${row.key}`);
    }
  }

  return {
    locale,
    editorialPolicy: {
      schemaVersion: editorialPolicy.schemaVersion,
      nvidiaFamilyTerms: editorialPolicy.nvidiaFamilyTerms[locale],
      nvidiaSource: editorialPolicy.nvidiaSources[locale],
      launcherProductNames: editorialPolicy.launcherProductNames,
      protectedTokens: editorialPolicy.protectedTokens,
      technicalOnlyStaticKeys: editorialPolicy.technicalOnlyStaticKeys,
      nvapiVerbatimValues: editorialPolicy.nvapiVerbatimValues,
      nvapiSemanticTranslations: editorialPolicy.nvapiSemanticTranslations[locale],
      typography: editorialPolicy.localeTypography[locale],
      chineseScriptRules: editorialPolicy.chineseScriptRules[locale] ?? null,
    },
    messages,
  };
}

function tsvCell(value) {
  return JSON.stringify(String(value));
}

export function formatReviewReport(report, format) {
  if (format === 'json') {
    return `${JSON.stringify(report, null, 2)}\n`;
  }
  if (format !== 'tsv') {
    fail(`unsupported format ${JSON.stringify(format)}`);
  }
  const policy = JSON.stringify(report.editorialPolicy);
  const rows = report.messages.map(({ key, context, source, translation }) =>
    [key, context, source, translation, policy].map(tsvCell).join('\t'),
  );
  return (
    [['key', 'context', 'source', 'translation', 'editorial_policy'].join('\t'), ...rows].join(
      '\n',
    ) + '\n'
  );
}
