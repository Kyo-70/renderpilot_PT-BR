import { describe, expect, it } from 'vitest';

import { lumaOverrides as de } from './de';
import { lumaOverrides as es } from './es';
import { lumaOverrides as fr } from './fr';
import { lumaOverrides as ja } from './ja';
import { lumaOverrides as ru } from './ru';
import { lumaOverrides as zhHans } from './zh-Hans';
import { lumaOverrides as zhHant } from './zh-Hant';
import { LUMA_SOURCE_CATALOG } from './contract.generated';

const catalogs = { de, es, fr, ja, ru, 'zh-Hans': zhHans, 'zh-Hant': zhHant } as const;

describe('Luma localized guidance messages', () => {
  it('contains every reviewed source message in every supported locale', () => {
    const expectedKeys = Object.keys(LUMA_SOURCE_CATALOG).toSorted();
    expect(expectedKeys.length).toBeGreaterThan(0);
    for (const [locale, catalog] of Object.entries(catalogs)) {
      expect(Object.keys(catalog).toSorted(), locale).toEqual(expectedKeys);
      for (const [key, translation] of Object.entries(catalog)) {
        expect(translation.trim(), `${locale}: ${key}`).not.toBe('');
      }
    }
  });

  it('does not silently ship English source prose as a translation', () => {
    for (const [locale, catalog] of Object.entries(catalogs)) {
      for (const [key, source] of Object.entries(LUMA_SOURCE_CATALOG)) {
        expect(catalog[key as keyof typeof catalog], `${locale}: ${key}`).not.toBe(source);
      }
    }
  });
});
