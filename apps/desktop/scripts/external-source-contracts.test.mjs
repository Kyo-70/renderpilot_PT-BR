import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';

import { parseExternalSourceCheckArguments } from './external-source-check/arguments.mjs';
import {
  verifyLumaSourceContract,
  verifyNvapiSourceContract,
  verifyOptiscalerSourceContract,
} from './external-source-contracts.mjs';
import { projectLumaI18nSource } from './luma-i18n-sync.mjs';
import { projectOptiscalerI18nSource } from './optiscaler-i18n-sync.mjs';

const lumaContract = {
  schemaVersion: 1,
  messages: [
    {
      id: 'luma.game.warning',
      sourceText: 'Careful.',
      kind: 'warning',
      context: 'guidance.warning',
    },
  ],
};
const lumaProducer = {
  schema_version: 1,
  messages: [
    {
      id: 'luma.game.warning',
      fallback_text: 'Careful.',
      kind: 'warning',
      context: 'guidance.warning',
      translations: {
        de: 'Vorsichtig.',
        es: 'Cuidado.',
        fr: 'Attention.',
        ja: '注意。',
        'pt-BR': 'Cuidado.',
        ru: 'Осторожно.',
        'zh-Hans': '小心。',
        'zh-Hant': '小心。',
      },
    },
  ],
};

test('source-check CLI requires exactly one non-empty producer root', () => {
  assert.deepEqual(parseExternalSourceCheckArguments(['--producer-root', './producer']), {
    producerRoot: path.resolve('./producer'),
  });
  for (const args of [
    [],
    ['--producer-root'],
    ['--producer-root', ''],
    ['--root', './producer'],
    ['--producer-root', './producer', '--extra', 'value'],
  ]) {
    assert.throws(() => parseExternalSourceCheckArguments(args), /Usage:/);
  }
});

test('Luma source check requires the exact reviewed message tuple', () => {
  assert.deepEqual(verifyLumaSourceContract(lumaContract, lumaProducer), { messageCount: 1 });
  assert.throws(
    () => verifyLumaSourceContract(lumaContract, { ...lumaProducer, messages: [] }),
    /message count differs/,
  );
  assert.throws(
    () =>
      verifyLumaSourceContract(lumaContract, {
        ...lumaProducer,
        messages: [{ ...lumaProducer.messages[0], fallback_text: 'Changed.' }],
      }),
    /changed/,
  );
  assert.throws(
    () =>
      verifyLumaSourceContract(lumaContract, {
        ...lumaProducer,
        messages: [{ ...lumaProducer.messages[0], context: 'guidance.compatibility' }],
      }),
    /changed/,
  );
  assert.throws(
    () =>
      verifyLumaSourceContract(lumaContract, {
        ...lumaProducer,
        messages: [lumaProducer.messages[0], lumaProducer.messages[0]],
      }),
    /duplicate Luma message ID/,
  );
});

test('Luma i18n projection requires one complete canonical producer catalog', () => {
  const translations = {
    de: 'Hinweis.',
    es: 'Aviso.',
    fr: 'Conseil.',
    ja: '案内。',
    'pt-BR': 'Orientação.',
    ru: 'Подсказка.',
    'zh-Hans': '提示。',
    'zh-Hant': '提示。',
  };
  const producer = {
    schema_version: 1,
    messages: [
      {
        id: 'luma.game.note',
        fallback_text: 'Guidance.',
        kind: 'compatibility',
        context: 'guidance.compatibility',
        translations,
      },
    ],
  };

  assert.deepEqual(projectLumaI18nSource(producer), {
    contract: {
      schemaVersion: 1,
      messages: [
        {
          id: 'luma.game.note',
          sourceText: 'Guidance.',
          kind: 'compatibility',
          context: 'guidance.compatibility',
        },
      ],
    },
    localized: Object.fromEntries(
      Object.entries(translations).map(([locale, translation]) => [
        locale,
        { 'luma.game.note': translation },
      ]),
    ),
  });

  const missingLocale = { ...translations };
  delete missingLocale.de;
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], translations: missingLocale }],
      }),
    /exact locale coverage/,
  );
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [
          {
            id: 'luma.legacy.guidance',
            fallback_text: 'Legacy fallback',
            guidance_kind: 'compatibility',
            context: 'guidance.compatibility',
            translations,
          },
        ],
      }),
    /message luma\.legacy\.guidance kind must be a non-empty string/,
  );
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], id: 'bad_id' }],
      }),
    /invalid message ID/,
  );
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], fallback_text: 'Hello {name}' }],
      }),
    /must not contain placeholders/,
  );
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], kind: 'engine_ini', context: 'guidance.warning' }],
      }),
    /does not match context/,
  );
  assert.throws(
    () =>
      projectLumaI18nSource({
        ...producer,
        messages: [
          { ...producer.messages[0], translations: { ...translations, de: 'Hallo {name}' } },
        ],
      }),
    /must not contain placeholders/,
  );
});

test('Luma source check and projection accept availability messages with blocked context', () => {
  const translations = {
    de: 'Gesperrt.',
    es: 'Bloqueado.',
    fr: 'Bloqué.',
    ja: '利用不可。',
    'pt-BR': 'Bloqueado.',
    ru: 'Заблокировано.',
    'zh-Hans': '已封禁。',
    'zh-Hant': '已封鎖。',
  };
  const blockedContract = {
    schemaVersion: 1,
    messages: [
      {
        id: 'luma.blocked-game.availability',
        sourceText: 'This Luma profile is unavailable.',
        kind: 'blocked',
        context: 'availability.blocked',
      },
    ],
  };
  const blockedProducer = {
    schema_version: 1,
    messages: [
      {
        id: 'luma.blocked-game.availability',
        fallback_text: 'This Luma profile is unavailable.',
        kind: 'blocked',
        context: 'availability.blocked',
        translations,
      },
    ],
  };

  assert.deepEqual(verifyLumaSourceContract(blockedContract, blockedProducer), { messageCount: 1 });
  assert.deepEqual(projectLumaI18nSource(blockedProducer), {
    contract: blockedContract,
    localized: Object.fromEntries(
      Object.entries(translations).map(([locale, translation]) => [
        locale,
        { 'luma.blocked-game.availability': translation },
      ]),
    ),
  });
});

const setting = {
  key: 'dlss_sr_mode',
  family: 'sr',
  label: 'Mode',
  description: 'Select a mode.',
  values: [{ wire: 'on', label: 'On' }],
};

test('NVAPI source check compares supported families and ignores producer-only NR', () => {
  const bundled = { schema_version: 1, settings: [setting] };
  const producer = {
    schema_version: 1,
    settings: [setting, { key: 'neural_rendering', family: 'nr', label: 'Neural Rendering' }],
  };
  assert.deepEqual(verifyNvapiSourceContract(bundled, producer), {
    settingCount: 1,
    messageCount: 3,
  });
  assert.throws(
    () =>
      verifyNvapiSourceContract(bundled, {
        schema_version: 1,
        settings: [{ ...setting, description: 'Changed.' }],
      }),
    /source text changed/,
  );
  assert.throws(
    () => verifyNvapiSourceContract(bundled, { schema_version: 1, settings: [] }),
    /stale message/,
  );
  assert.throws(
    () =>
      verifyNvapiSourceContract(bundled, {
        schema_version: 1,
        settings: [setting, { ...setting }],
      }),
    /duplicate NVAPI setting key/,
  );
  assert.throws(
    () =>
      verifyNvapiSourceContract(bundled, {
        schema_version: 1,
        settings: [{ ...setting, label: 'Mode {name}' }],
      }),
    /must not contain placeholders/,
  );
});

test('OptiScaler source check requires the exact reviewed message tuple', () => {
  const contract = {
    schemaVersion: 1,
    messages: [
      {
        id: 'optiscaler-game-note',
        sourceText: 'Use the supported input.',
        kind: 'compatibility',
        context: 'compatibility',
      },
    ],
  };
  const producer = {
    schema_version: 1,
    messages: [
      {
        id: 'optiscaler-game-note',
        fallback_text: 'Use the supported input.',
        guidance_kind: 'compatibility',
        context: 'compatibility',
        translations: {
          de: 'Unterstützte Eingabe verwenden.',
          es: 'Usa la entrada compatible.',
          fr: 'Utilisez l’entrée prise en charge.',
          ja: '対応している入力を使用してください。',
          'pt-BR': 'Use a entrada compatível.',
          ru: 'Используйте поддерживаемый вход.',
          'zh-Hans': '使用受支持的输入。',
          'zh-Hant': '使用支援的輸入。',
        },
      },
    ],
  };

  assert.deepEqual(verifyOptiscalerSourceContract(contract, producer), {
    messageCount: 1,
  });
  assert.throws(
    () =>
      verifyOptiscalerSourceContract(contract, {
        ...producer,
        messages: [{ ...producer.messages[0], fallback_text: 'Changed text.' }],
      }),
    /changed/,
  );
  assert.throws(
    () => verifyOptiscalerSourceContract(contract, { ...producer, messages: [] }),
    /message count differs/,
  );
});

test('OptiScaler i18n projection requires one complete canonical producer catalog', () => {
  const translations = {
    de: 'Hinweis.',
    es: 'Aviso.',
    fr: 'Conseil.',
    ja: '案内。',
    'pt-BR': 'Orientação.',
    ru: 'Подсказка.',
    'zh-Hans': '提示。',
    'zh-Hant': '提示。',
  };
  const producer = {
    schema_version: 1,
    messages: [
      {
        id: 'optiscaler-game-note',
        fallback_text: 'Guidance.',
        guidance_kind: 'compatibility',
        context: 'compatibility',
        translations,
      },
    ],
  };

  assert.deepEqual(projectOptiscalerI18nSource(producer), {
    contract: {
      schemaVersion: 1,
      messages: [
        {
          id: 'optiscaler-game-note',
          sourceText: 'Guidance.',
          kind: 'compatibility',
          context: 'compatibility',
        },
      ],
    },
    localized: Object.fromEntries(
      Object.entries(translations).map(([locale, translation]) => [
        locale,
        { 'optiscaler-game-note': translation },
      ]),
    ),
  });

  const missingLocale = { ...translations };
  delete missingLocale.de;
  assert.throws(
    () =>
      projectOptiscalerI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], translations: missingLocale }],
      }),
    /exact locale coverage/,
  );
  assert.throws(
    () =>
      projectOptiscalerI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], id: 'bad-id-without-prefix' }],
      }),
    /invalid or duplicate OptiScaler message id/,
  );
  assert.throws(
    () =>
      projectOptiscalerI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], fallback_text: 'Hello {name}' }],
      }),
    /must not contain placeholders/,
  );
  assert.throws(
    () =>
      projectOptiscalerI18nSource({
        ...producer,
        messages: [{ ...producer.messages[0], guidance_kind: 'unknown' }],
      }),
    /invalid kind\/context/,
  );
  assert.throws(
    () =>
      projectOptiscalerI18nSource({
        ...producer,
        messages: [
          { ...producer.messages[0], translations: { ...translations, de: 'Hallo {name}' } },
        ],
      }),
    /must not contain placeholders/,
  );
});
