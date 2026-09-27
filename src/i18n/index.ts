// Tradução da interface. Todo texto visível vem de ./locales. Para um idioma
// novo, crie o JSON com as mesmas chaves e registre-o em `resources`.
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import en from "./locales/en.json";
import ptBR from "./locales/pt-BR.json";

import { FALLBACK_LANGUAGE, pickLanguage, type Language } from "./pick-language";

export const resources: Record<Language, { translation: typeof ptBR }> = {
  "pt-BR": { translation: ptBR },
  en: { translation: en },
};

void i18n.use(initReactI18next).init({
  resources,
  lng: pickLanguage(navigator.languages ?? [navigator.language]),
  fallbackLng: FALLBACK_LANGUAGE,
  interpolation: { escapeValue: false },
});

document.documentElement.lang = i18n.language;

export default i18n;
