import { createContext, useContext, useMemo } from "react";
import {
  format,
  messagesFor,
  resolveUiLanguage,
  type MessageKey,
  type UiLanguage,
  type UiLanguageSetting,
} from "./index";

export interface I18n {
  lang: UiLanguage;
  t: (key: MessageKey, params?: Record<string, string | number>) => string;
  /** Picks between a singular and a plural key and passes `count` through. */
  tn: (one: MessageKey, many: MessageKey, count: number) => string;
}

function build(setting: UiLanguageSetting): I18n {
  const lang = resolveUiLanguage(setting);
  const messages = messagesFor(lang);
  return {
    lang,
    t: (key, params) => format(messages, key, params),
    tn: (one, many, count) => format(messages, count === 1 ? one : many, { count }),
  };
}

/**
 * English until a provider mounts. Nothing in the app relies on this default —
 * it only keeps `useT` usable outside the tree.
 */
export const I18nContext = createContext<I18n>(build("en"));

/**
 * The value `App` puts on the context. `App` also needs `t` for its own status
 * messages, so it builds the value itself rather than reading it back out of a
 * provider it renders — a component cannot consume its own context.
 */
export function useI18nValue(setting: UiLanguageSetting): I18n {
  return useMemo(() => build(setting), [setting]);
}

export function useT(): I18n {
  return useContext(I18nContext);
}
