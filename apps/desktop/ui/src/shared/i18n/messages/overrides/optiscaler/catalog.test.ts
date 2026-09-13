import { describe, expect, it } from 'vitest';

import { optiscalerOverrides as de } from './de';
import { optiscalerOverrides as es } from './es';
import { optiscalerOverrides as fr } from './fr';
import { optiscalerOverrides as ja } from './ja';
import { optiscalerOverrides as ru } from './ru';
import { optiscalerOverrides as zhHans } from './zh-Hans';
import { optiscalerOverrides as zhHant } from './zh-Hant';
import { OPTISCALER_SOURCE_CATALOG } from './contract.generated';

const catalogs = { de, es, fr, ja, ru, 'zh-Hans': zhHans, 'zh-Hant': zhHant } as const;

describe('OptiScaler localized compatibility messages', () => {
  it('contains every reviewed source message in every supported locale', () => {
    const expectedKeys = Object.keys(OPTISCALER_SOURCE_CATALOG).toSorted();
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
      for (const [key, source] of Object.entries(OPTISCALER_SOURCE_CATALOG)) {
        expect(catalog[key as keyof typeof catalog], `${locale}: ${key}`).not.toBe(source);
      }
    }
  });
});
