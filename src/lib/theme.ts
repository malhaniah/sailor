// Theme presets following Harbor's token-override model: a preset is a map of the
// twelve --color-* tokens, applied as inline CSS vars on <html>. Palette values for
// nord/dracula/noir/forest follow Harbor's presets (MIT).
export interface ThemePreset {
  id: string;
  name: string;
  blurb: string;
  /** [canvas, gradient-top, accent/text] used by the picker tile */
  swatch: [string, string, string];
  tokens: Record<string, string>;
}

export const THEME_PRESETS: ThemePreset[] = [
  {
    id: "harbor-grey",
    name: "Zeiler default",
    blurb: "Cool grey with warm gold.",
    swatch: ["#2c2e36", "#3a3d47", "#dcdde4"],
    tokens: {}, // the @theme defaults
  },
  {
    id: "nord",
    name: "Nord",
    blurb: "Arctic, north-bluish calm.",
    swatch: ["#2e3440", "#3b4252", "#88c0d0"],
    tokens: {
      "--color-canvas": "#2e3440",
      "--color-surface": "#353a47",
      "--color-elevated": "#3b4252",
      "--color-raised": "#434c5e",
      "--color-ink": "#eceff4",
      "--color-ink-muted": "#d8dee9",
      "--color-ink-subtle": "#8b95a4",
      "--color-edge": "#4c566a8c",
      "--color-edge-soft": "#4c566a40",
      "--color-accent": "#88c0d0",
      "--color-accent-soft": "#88c0d02e",
      "--color-danger": "#bf616a",
    },
  },
  {
    id: "dracula",
    name: "Dracula",
    blurb: "Purple-tinted night.",
    swatch: ["#282a36", "#44475a", "#bd93f9"],
    tokens: {
      "--color-canvas": "#282a36",
      "--color-surface": "#21222c",
      "--color-elevated": "#44475a",
      "--color-raised": "#565969",
      "--color-ink": "#f8f8f2",
      "--color-ink-muted": "#d6d6d0",
      "--color-ink-subtle": "#6272a4",
      "--color-edge": "#6272a48c",
      "--color-edge-soft": "#6272a440",
      "--color-accent": "#bd93f9",
      "--color-accent-soft": "#bd93f92e",
      "--color-danger": "#ff5555",
    },
  },
  {
    id: "forest",
    name: "Forest",
    blurb: "Deep greens, mossy accent.",
    swatch: ["#13211a", "#1c2f24", "#7ed6a2"],
    tokens: {
      "--color-canvas": "oklch(0.18 0.03 145)",
      "--color-surface": "oklch(0.22 0.03 145)",
      "--color-elevated": "oklch(0.26 0.03 145)",
      "--color-raised": "oklch(0.31 0.03 145)",
      "--color-ink": "oklch(0.97 0.01 140)",
      "--color-ink-muted": "oklch(0.74 0.02 140)",
      "--color-ink-subtle": "oklch(0.52 0.02 140)",
      "--color-edge": "oklch(0.36 0.03 145 / 0.55)",
      "--color-edge-soft": "oklch(0.36 0.03 145 / 0.25)",
      "--color-accent": "oklch(0.8 0.15 145)",
      "--color-accent-soft": "oklch(0.8 0.15 145 / 0.18)",
      "--color-danger": "oklch(0.58 0.2 25)",
    },
  },
  {
    id: "noir",
    name: "Noir",
    blurb: "Pure black and white.",
    swatch: ["#000000", "#0e0e0e", "#f5f5f5"],
    tokens: {
      "--color-canvas": "#000000",
      "--color-surface": "#070707",
      "--color-elevated": "#0e0e0e",
      "--color-raised": "#1a1a1a",
      "--color-ink": "#f5f5f5",
      "--color-ink-muted": "#9a9a9a",
      "--color-ink-subtle": "#555555",
      "--color-edge": "rgba(255,255,255,0.08)",
      "--color-edge-soft": "rgba(255,255,255,0.03)",
      "--color-accent": "#ffffff",
      "--color-accent-soft": "rgba(255,255,255,0.18)",
      "--color-danger": "#b94545",
    },
  },
];

const ALL_TOKENS = [
  "--color-canvas",
  "--color-surface",
  "--color-elevated",
  "--color-raised",
  "--color-ink",
  "--color-ink-muted",
  "--color-ink-subtle",
  "--color-edge",
  "--color-edge-soft",
  "--color-accent",
  "--color-accent-soft",
  "--color-danger",
];

export function applyTheme(presetId: string) {
  const preset = THEME_PRESETS.find((p) => p.id === presetId) ?? THEME_PRESETS[0];
  const root = document.documentElement;
  for (const token of ALL_TOKENS) {
    const value = preset.tokens[token];
    if (value) root.style.setProperty(token, value);
    else root.style.removeProperty(token);
  }
}
