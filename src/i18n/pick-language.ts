// Escolha do idioma, separada do index.ts para ser testável sem o navegador.

export const SUPPORTED_LANGUAGES = ["pt-BR", "en"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

/** Idioma usado quando o do sistema não é suportado. */
export const FALLBACK_LANGUAGE: Language = "en";

/** Escolhe o idioma da interface a partir das preferências do sistema, em ordem. */
export function pickLanguage(preferred: readonly string[]): Language {
  for (const tag of preferred) {
    const lower = tag.toLowerCase();
    if (lower.startsWith("pt")) return "pt-BR";
    if (lower.startsWith("en")) return "en";
  }
  return FALLBACK_LANGUAGE;
}
