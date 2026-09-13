import {
  ExternalContractValidationError,
  projectSupportedNvapiCatalog as projectSupportedNvapiCatalogCore,
  validateLumaContract as validateLumaContractCore,
  validateOptiscalerContract as validateOptiscalerContractCore,
} from './external-contract-core.mjs';

function fail(message, cause) {
  throw new Error(`external i18n source check failed: ${message}`, { cause });
}

function validate(operation) {
  try {
    return operation();
  } catch (cause) {
    if (cause instanceof ExternalContractValidationError) {
      fail(cause.message, cause);
    }
    throw cause;
  }
}

function nonEmptyString(value, context) {
  if (typeof value !== 'string' || value.trim() === '') {
    fail(`${context} must be a non-empty string`);
  }
  return value;
}

/** Verifies the checked-in Luma source contract against the producer's
 * reviewed guidance-message contract. */
export function verifyLumaSourceContract(contract, producer) {
  validate(() => validateLumaContractCore(contract));
  if (producer?.schema_version !== 1 || !Array.isArray(producer.messages)) {
    fail('Luma producer message contract has an unsupported shape');
  }
  const project = (messages, sourceName) => {
    const result = new Map();
    for (const message of messages) {
      const id = nonEmptyString(message?.id, `${sourceName} message id`);
      const sourceText = nonEmptyString(
        message?.sourceText ?? message?.fallback_text,
        `${sourceName} message ${id} source text`,
      );
      const kind = nonEmptyString(message?.kind, `${sourceName} message ${id} kind`);
      const context = nonEmptyString(message?.context, `${sourceName} message ${id} context`);
      if (result.has(id)) {
        fail(`duplicate Luma message ID ${id}`);
      }
      result.set(id, { sourceText, kind, context });
    }
    return result;
  };
  const checked = project(contract.messages, 'checked-in');
  const actual = project(producer.messages, 'producer');
  if (checked.size !== actual.size) {
    fail(
      `Luma source contract message count differs from producer (${checked.size} vs ${actual.size})`,
    );
  }
  for (const [id, expected] of actual) {
    const candidate = checked.get(id);
    if (candidate === undefined) {
      fail(`Luma source contract is missing producer message ${id}`);
    }
    if (
      candidate.sourceText !== expected.sourceText ||
      candidate.kind !== expected.kind ||
      candidate.context !== expected.context
    ) {
      fail(`Luma source contract changed for ${id}`);
    }
  }
  return { messageCount: actual.size };
}

export function projectSupportedNvapiCatalog(value) {
  return validate(() => projectSupportedNvapiCatalogCore(value));
}

export function verifyNvapiSourceContract(bundled, producer) {
  return validate(() => {
    const expected = projectSupportedNvapiCatalogCore(bundled);
    const actual = projectSupportedNvapiCatalogCore(producer);
    const expectedKeys = new Set(Object.keys(expected.sourceCatalog));
    const actualKeys = new Set(Object.keys(actual.sourceCatalog));

    for (const key of actualKeys.difference(expectedKeys)) {
      fail(`bundled NVAPI catalog is missing producer message ${key}`);
    }
    for (const key of expectedKeys.difference(actualKeys)) {
      fail(`bundled NVAPI catalog contains stale message ${key}`);
    }
    for (const key of expectedKeys.intersection(actualKeys)) {
      if (expected.sourceCatalog[key] !== actual.sourceCatalog[key]) {
        fail(`NVAPI source text changed for ${key}`);
      }
    }

    return { settingCount: actual.settingCount, messageCount: actualKeys.size };
  });
}

/** Verifies the checked-in OptiScaler source contract against the producer's
 * reviewed compatibility-message contract. */
export function verifyOptiscalerSourceContract(contract, producer) {
  validate(() => validateOptiscalerContractCore(contract));
  if (producer?.schema_version !== 1 || !Array.isArray(producer.messages)) {
    fail('OptiScaler producer message contract has an unsupported shape');
  }
  const project = (messages, sourceName) => {
    const result = new Map();
    for (const message of messages) {
      const id = nonEmptyString(message?.id, `${sourceName} message id`);
      const sourceText = nonEmptyString(
        message?.sourceText ?? message?.fallback_text,
        `${sourceName} message ${id} source text`,
      );
      const kind = nonEmptyString(
        message?.kind ?? message?.guidance_kind,
        `${sourceName} message ${id} kind`,
      );
      const context = nonEmptyString(message?.context, `${sourceName} message ${id} context`);
      if (result.has(id)) {
        fail(`duplicate OptiScaler message ID ${id}`);
      }
      result.set(id, { sourceText, kind, context });
    }
    return result;
  };
  const checked = project(contract.messages, 'checked-in');
  const actual = project(producer.messages, 'producer');
  if (checked.size !== actual.size) {
    fail(
      `OptiScaler source contract message count differs from producer (${checked.size} vs ${actual.size})`,
    );
  }
  for (const [id, expected] of actual) {
    const candidate = checked.get(id);
    if (candidate === undefined) {
      fail(`OptiScaler source contract is missing producer message ${id}`);
    }
    if (
      candidate.sourceText !== expected.sourceText ||
      candidate.kind !== expected.kind ||
      candidate.context !== expected.context
    ) {
      fail(`OptiScaler source contract changed for ${id}`);
    }
  }
  return { messageCount: actual.size };
}
