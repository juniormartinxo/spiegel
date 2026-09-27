import { describe, expect, it } from "vitest";

import en from "./locales/en.json";
import ptBR from "./locales/pt-BR.json";
import { pickLanguage } from "./pick-language";

function keys(value: unknown, prefix = ""): string[] {
  if (typeof value !== "object" || value === null) return [prefix];
  return Object.entries(value).flatMap(([key, child]) => keys(child, prefix ? `${prefix}.${key}` : key));
}

describe("traduções", () => {
  it("pt-BR e inglês têm exatamente as mesmas chaves", () => {
    expect(keys(en).sort()).toEqual(keys(ptBR).sort());
  });

  it("nenhum texto está vazio", () => {
    for (const locale of [en, ptBR]) {
      for (const key of keys(locale)) {
        const text = key.split(".").reduce<unknown>((node, part) => (node as Record<string, unknown>)[part], locale);
        expect(text, key).not.toBe("");
      }
    }
  });
});

describe("pickLanguage", () => {
  it("português de qualquer região abre em pt-BR", () => {
    expect(pickLanguage(["pt-BR"])).toBe("pt-BR");
    expect(pickLanguage(["pt-PT", "en-US"])).toBe("pt-BR");
  });

  it("inglês abre em inglês", () => {
    expect(pickLanguage(["en-GB"])).toBe("en");
  });

  it("idioma não suportado cai para o inglês", () => {
    expect(pickLanguage(["es-ES", "fr-FR"])).toBe("en");
    expect(pickLanguage([])).toBe("en");
  });

  it("respeita a ordem de preferência", () => {
    expect(pickLanguage(["es-ES", "pt-BR", "en-US"])).toBe("pt-BR");
  });
});
