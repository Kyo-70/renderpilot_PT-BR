import { describe, expect, it } from 'vitest';

import { renodxOverrides as de } from './de';
import { renodxOverrides as es } from './es';
import { renodxOverrides as fr } from './fr';
import { renodxOverrides as ja } from './ja';
import { renodxOverrides as ptBr } from './pt-BR';
import { renodxOverrides as ru } from './ru';
import { renodxOverrides as zhHans } from './zh-Hans';
import { renodxOverrides as zhHant } from './zh-Hant';
import { RENODX_SOURCE_CATALOG } from './contract.generated';

const catalogs = {
  de,
  es,
  fr,
  ja,
  'pt-BR': ptBr,
  ru,
  'zh-Hans': zhHans,
  'zh-Hant': zhHant,
} as const;

describe('RenoDX localized guidance messages', () => {
  it('contains every reviewed source message in every supported locale', () => {
    const expectedKeys = Object.keys(RENODX_SOURCE_CATALOG).toSorted();
    expect(expectedKeys.length).toBeGreaterThan(0);
    for (const [locale, catalog] of Object.entries(catalogs)) {
      expect(Object.keys(catalog).toSorted(), locale).toEqual(expectedKeys);
      for (const [key, translation] of Object.entries(catalog)) {
        expect(translation.trim(), `${locale}: ${key}`).not.toBe('');
      }
    }
  });

  it('Russian locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = ru[key as keyof typeof ru];
      expect(translation, `ru: ${key}`).toBeDefined();
      expect(translation.trim(), `ru: ${key}`).not.toBe('');
    }
  });

  it('German locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = de[key as keyof typeof de];
      expect(translation, `de: ${key}`).toBeDefined();
      expect(translation.trim(), `de: ${key}`).not.toBe('');
    }
  });

  it('French locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = fr[key as keyof typeof fr];
      expect(translation, `fr: ${key}`).toBeDefined();
      expect(translation.trim(), `fr: ${key}`).not.toBe('');
    }
  });

  it('Spanish locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = es[key as keyof typeof es];
      expect(translation, `es: ${key}`).toBeDefined();
      expect(translation.trim(), `es: ${key}`).not.toBe('');
    }
  });

  it('Japanese locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = ja[key as keyof typeof ja];
      expect(translation, `ja: ${key}`).toBeDefined();
      expect(translation.trim(), `ja: ${key}`).not.toBe('');
    }
  });

  it('Brazilian Portuguese locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = ptBr[key as keyof typeof ptBr];
      expect(translation, `pt-BR: ${key}`).toBeDefined();
      expect(translation.trim(), `pt-BR: ${key}`).not.toBe('');
    }
  });

  it('Simplified Chinese locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = zhHans[key as keyof typeof zhHans];
      expect(translation, `zh-Hans: ${key}`).toBeDefined();
      expect(translation.trim(), `zh-Hans: ${key}`).not.toBe('');
    }
  });

  it('Traditional Chinese locale contains complete translations', () => {
    for (const key of Object.keys(RENODX_SOURCE_CATALOG)) {
      const translation = zhHant[key as keyof typeof zhHant];
      expect(translation, `zh-Hant: ${key}`).toBeDefined();
      expect(translation.trim(), `zh-Hant: ${key}`).not.toBe('');
    }
  });
});
