import { analyzeMessageTemplate } from '../ui/src/shared/i18n/messages/template.ts';

const LUMA_CONTEXT_PATTERN = /^(?:guidance\.[a-z][a-z0-9_]*|availability\.blocked)$/;
const LUMA_MESSAGE_ID_PATTERN = /^luma\.[a-z0-9-]+\.[a-z0-9_-]+$/;
const RENODX_CONTEXT_PATTERN = /^guidance\.[a-z][a-z0-9_]*$/;
const RENODX_MESSAGE_ID_PATTERN = /^renodx\.[a-z0-9_-]+(?:\.[a-z0-9_-]+)*$/;
export const RENODX_KINDS = Object.freeze([
  'engine_ini',
  'compatibility',
  'warning',
  'external_tool',
  'game_setting',
  'addon_setting',
]);
const RENODX_KIND_SET = new Set(RENODX_KINDS);
const OPTISCALER_MESSAGE_ID_PATTERN = /^optiscaler-[a-z0-9-]+$/u;
const OPTISCALER_KINDS = new Set(['compatibility', 'game_setting']);
const NVAPI_IDENTIFIER_PATTERN = /^[a-z0-9_]+$/;

const SUPPORTED_NVAPI_FAMILIES = new Set(['sr', 'fg', 'rr']);

export class ExternalContractValidationError extends Error {
  constructor(message) {
    super(message);
    this.name = 'ExternalContractValidationError';
  }
}

function fail(message) {
  throw new ExternalContractValidationError(message);
}

function assertRecord(value, context) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    fail(`${context} must be an object`);
  }
}

function assertExactKeys(value, keys, context) {
  assertRecord(value, context);
  const unknownKeys = Object.keys(value).filter((key) => !keys.includes(key));
  if (unknownKeys.length > 0) {
    fail(`${context} contains unknown field ${JSON.stringify(unknownKeys[0])}`);
  }
  const missingKeys = keys.filter((key) => !Object.hasOwn(value, key));
  if (missingKeys.length > 0) {
    fail(`${context} is missing field ${JSON.stringify(missingKeys[0])}`);
  }
}

function nonEmptyString(value, context) {
  if (typeof value !== 'string' || value.trim() === '') {
    fail(`${context} must be a non-empty string`);
  }
  return value;
}

function assertExternalSourceText(value, context) {
  const source = nonEmptyString(value, context);
  const template = analyzeMessageTemplate(source);
  if (!template.valid) {
    fail(`${context} contains invalid placeholder syntax`);
  }
  if (template.placeholders.length > 0) {
    fail(`${context} must not contain placeholders`);
  }
  return source;
}

function sortRecord(record) {
  return Object.fromEntries(
    Object.entries(record).sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0)),
  );
}

/** Validates the checked-in, reviewed Luma translation contract. */
export function validateLumaContract(value) {
  assertExactKeys(value, ['schemaVersion', 'messages'], 'Luma contract');
  if (value.schemaVersion !== 1) {
    fail(`Luma contract has unsupported schemaVersion ${JSON.stringify(value.schemaVersion)}`);
  }
  if (!Array.isArray(value.messages)) {
    fail('Luma contract messages must be an array');
  }

  const sourceCatalog = {};
  const contexts = {};

  for (const message of value.messages) {
    assertExactKeys(message, ['id', 'sourceText', 'kind', 'context'], 'Luma message');
    const id = nonEmptyString(message.id, 'Luma message ID');
    if (!LUMA_MESSAGE_ID_PATTERN.test(id)) {
      fail(`Luma message has invalid message ID ${JSON.stringify(id)}`);
    }
    if (Object.hasOwn(sourceCatalog, id)) {
      fail(`duplicate Luma message ID ${id}`);
    }
    const sourceText = assertExternalSourceText(
      message.sourceText,
      `Luma message ${id} sourceText`,
    );
    const kind = nonEmptyString(message.kind, `Luma message ${id} kind`);
    const context = nonEmptyString(message.context, `Luma message ${id} context`);
    if (!LUMA_CONTEXT_PATTERN.test(context)) {
      fail(`Luma message ${id} has invalid context ${JSON.stringify(context)}`);
    }
    if (context === 'availability.blocked') {
      if (kind !== 'blocked') {
        fail(
          `Luma message ${id} with context ${JSON.stringify(context)} must have kind "blocked", got ${JSON.stringify(kind)}`,
        );
      }
    } else if (context !== `guidance.${kind}`) {
      fail(
        `Luma message ${id} kind ${JSON.stringify(kind)} does not match context ${JSON.stringify(context)}`,
      );
    }
    sourceCatalog[id] = sourceText;
    contexts[id] = context;
  }

  if (Object.keys(sourceCatalog).length === 0) {
    fail('Luma contract must contain at least one message');
  }
  return {
    sourceCatalog: sortRecord(sourceCatalog),
    contexts: sortRecord(contexts),
  };
}

/** Validates the checked-in, reviewed OptiScaler compatibility-message contract. */
export function validateOptiscalerContract(value) {
  assertExactKeys(value, ['schemaVersion', 'messages'], 'OptiScaler contract');
  if (value.schemaVersion !== 1) {
    fail(
      `OptiScaler contract has unsupported schemaVersion ${JSON.stringify(value.schemaVersion)}`,
    );
  }
  if (!Array.isArray(value.messages) || value.messages.length === 0) {
    fail('OptiScaler contract messages must be a non-empty array');
  }

  const sourceCatalog = {};
  const contexts = {};

  for (const message of value.messages) {
    assertExactKeys(message, ['id', 'sourceText', 'kind', 'context'], 'OptiScaler message');
    const id = nonEmptyString(message.id, 'OptiScaler message id');
    if (!OPTISCALER_MESSAGE_ID_PATTERN.test(id) || Object.hasOwn(sourceCatalog, id)) {
      fail(`invalid or duplicate OptiScaler message id ${JSON.stringify(id)}`);
    }
    const sourceText = assertExternalSourceText(
      message.sourceText,
      `OptiScaler message ${id} sourceText`,
    );
    const kind = nonEmptyString(message.kind, `OptiScaler message ${id} kind`);
    const context = nonEmptyString(message.context, `OptiScaler message ${id} context`);
    if (!OPTISCALER_KINDS.has(kind) || kind !== context) {
      fail(`OptiScaler message ${id} has an invalid kind/context`);
    }
    sourceCatalog[id] = sourceText;
    contexts[id] = context;
  }

  return {
    sourceCatalog: sortRecord(sourceCatalog),
    contexts: sortRecord(contexts),
  };
}

/** Validates the checked-in, reviewed RenoDX translation contract. */
export function validateRenodxContract(value) {
  assertExactKeys(value, ['schemaVersion', 'messages'], 'RenoDX contract');
  if (value.schemaVersion !== 1) {
    fail(`RenoDX contract has unsupported schemaVersion ${JSON.stringify(value.schemaVersion)}`);
  }
  if (!Array.isArray(value.messages)) {
    fail('RenoDX contract messages must be an array');
  }

  const sourceCatalog = {};
  const contexts = {};

  for (const message of value.messages) {
    assertExactKeys(message, ['id', 'sourceText', 'kind', 'context'], 'RenoDX message');
    const id = nonEmptyString(message.id, 'RenoDX message ID');
    if (!RENODX_MESSAGE_ID_PATTERN.test(id)) {
      fail(`RenoDX message has invalid message ID ${JSON.stringify(id)}`);
    }
    if (Object.hasOwn(sourceCatalog, id)) {
      fail(`duplicate RenoDX message ID ${id}`);
    }
    const sourceText = assertExternalSourceText(
      message.sourceText,
      `RenoDX message ${id} sourceText`,
    );
    const kind = nonEmptyString(message.kind, `RenoDX message ${id} kind`);
    if (!RENODX_KIND_SET.has(kind)) {
      fail(`RenoDX message ${id} has invalid kind ${JSON.stringify(kind)}`);
    }
    const context = nonEmptyString(message.context, `RenoDX message ${id} context`);
    if (!RENODX_CONTEXT_PATTERN.test(context)) {
      fail(`RenoDX message ${id} has invalid context ${JSON.stringify(context)}`);
    }
    if (context !== `guidance.${kind}`) {
      fail(
        `RenoDX message ${id} kind ${JSON.stringify(kind)} does not match context ${JSON.stringify(context)}`,
      );
    }
    sourceCatalog[id] = sourceText;
    contexts[id] = context;
  }

  if (Object.keys(sourceCatalog).length === 0) {
    fail('RenoDX contract must contain at least one message');
  }
  return {
    sourceCatalog: sortRecord(sourceCatalog),
    contexts: sortRecord(contexts),
  };
}

/** Extracts every producer-owned Luma message that may be shown to a user. */
export function projectLumaManifest(manifest) {
  assertRecord(manifest, 'Luma manifest');
  if (!Array.isArray(manifest.games)) {
    fail('Luma manifest must contain a games array');
  }

  const projection = {};
  const addMessage = (message, context) => {
    assertRecord(message, context);
    if (!LUMA_CONTEXT_PATTERN.test(context)) {
      fail(`invalid Luma context ${context}`);
    }
    const id = nonEmptyString(message.id, `${context}.id`);
    if (!LUMA_MESSAGE_ID_PATTERN.test(id)) {
      fail(`${context} has invalid message ID ${JSON.stringify(id)}`);
    }
    if (Object.hasOwn(projection, id)) {
      fail(`duplicate Luma message ID ${id}`);
    }
    projection[id] = {
      context,
      sourceText: assertExternalSourceText(message.fallback_text, `${context}.fallback_text`),
    };
  };

  for (const game of manifest.games) {
    assertRecord(game, 'Luma game');
    const gameId = nonEmptyString(game.id, 'Luma game.id');
    if (game.guidance !== undefined && !Array.isArray(game.guidance)) {
      fail(`Luma game ${gameId} guidance must be an array`);
    }
    for (const message of game.guidance ?? []) {
      const kind = nonEmptyString(message?.kind, `Luma game ${gameId} guidance.kind`);
      addMessage(message, `guidance.${kind}`);
    }
    if (game.availability?.message !== undefined) {
      const kind = nonEmptyString(game.availability.kind, `Luma game ${gameId} availability.kind`);
      addMessage(game.availability.message, `availability.${kind}`);
    }
  }

  return sortRecord(projection);
}

/** Projects a bundled/producer NVAPI catalog to the locale-aware UI contract. */
export function projectSupportedNvapiCatalog(value) {
  assertRecord(value, 'NVAPI catalog');
  if (value.schema_version !== 1) {
    fail(`NVAPI catalog has unsupported schema_version ${JSON.stringify(value.schema_version)}`);
  }
  if (!Array.isArray(value.settings)) {
    fail('NVAPI catalog must contain a settings array');
  }

  const settings = value.settings.filter((setting) =>
    SUPPORTED_NVAPI_FAMILIES.has(setting?.family),
  );
  const sourceCatalog = {};
  const settingKeys = new Set();

  const addMessage = (key, source) => {
    if (Object.hasOwn(sourceCatalog, key)) {
      fail(`duplicate NVAPI message key ${key}`);
    }
    sourceCatalog[key] = assertExternalSourceText(source, key);
  };

  for (const setting of settings) {
    assertRecord(setting, 'NVAPI setting');
    const key = nonEmptyString(setting.key, 'NVAPI setting key');
    if (!NVAPI_IDENTIFIER_PATTERN.test(key) || settingKeys.has(key)) {
      fail(`invalid or duplicate NVAPI setting key ${key}`);
    }
    settingKeys.add(key);

    const prefix = `nvapi.${key}`;
    addMessage(`${prefix}.label`, setting.label);
    if (setting.description !== undefined) {
      addMessage(`${prefix}.description`, setting.description);
    }
    if (setting.values !== undefined && !Array.isArray(setting.values)) {
      fail(`${key}.values must be an array`);
    }

    const wires = new Set();
    for (const option of setting.values ?? []) {
      assertRecord(option, `${key}.values entry`);
      const wire = nonEmptyString(option.wire, `${key}.value.wire`);
      if (!NVAPI_IDENTIFIER_PATTERN.test(wire) || wires.has(wire)) {
        fail(`invalid or duplicate wire value ${key}.${wire}`);
      }
      wires.add(wire);
      addMessage(`${prefix}.value.${wire}`, option.label);
    }
  }

  return {
    settingCount: settings.length,
    sourceCatalog: sortRecord(sourceCatalog),
  };
}
