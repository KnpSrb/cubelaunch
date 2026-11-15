// i18n. Чтобы добавить язык: положите locales/<код>.json с теми же ключами,
// что в ru.json, и добавьте запись в LANGUAGES ниже.

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import ru from "./locales/ru.json";
import en from "./locales/en.json";

export const LANGUAGES = [
  { code: "ru", name: "Русский" },
  { code: "en", name: "English" },
] as const;

export const DEFAULT_LANGUAGE = "ru";

void i18n.use(initReactI18next).init({
  resources: {
    ru: { translation: ru },
    en: { translation: en },
  },
  lng: DEFAULT_LANGUAGE,
  fallbackLng: DEFAULT_LANGUAGE,
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
