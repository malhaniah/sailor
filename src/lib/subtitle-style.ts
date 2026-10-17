/**
 * The subtitle look Settings offers: its choices (all mpv values, checked again by Rust
 * against the player's property rules) and a CSS approximation for the preview. mpv
 * (libass) draws the real subtitles; the preview only shows the idea.
 */
import type { CSSProperties } from "react";
import type { SubtitleStyle } from "./types";

/** The fonts the player allows (`FONTS` in src-tauri/src/commands/mpv/validate.rs; a
 * Rust test checks every one is listed there). */
export const SUBTITLE_FONTS = [
  "sans-serif",
  "serif",
  "monospace",
  "Arial",
  "Helvetica",
  "Verdana",
  "Tahoma",
  "Trebuchet MS",
  "Segoe UI",
  "Georgia",
  "Times New Roman",
  "Courier New",
  "Noto Sans",
  "DejaVu Sans",
  "Liberation Sans",
  "Roboto",
] as const;

/** A semi-transparent black box (mpv colours are #AARRGGBB, alpha first). */
export const BOX_COLOR = "#99000000";

export const STYLE_CHOICES = {
  scale: [
    { value: null, label: "Default" },
    { value: 0.8, label: "Small" },
    { value: 1.25, label: "Large" },
    { value: 1.5, label: "Larger" },
    { value: 1.8, label: "Huge" },
  ],
  color: [
    { value: null, label: "Default" },
    { value: "#FFFFFF", label: "White" },
    { value: "#FFE14D", label: "Yellow" },
    { value: "#5FE3FF", label: "Cyan" },
  ],
  outlineSize: [
    { value: null, label: "Default" },
    { value: 0, label: "None" },
    { value: 1, label: "Thin" },
    { value: 2, label: "Medium" },
    { value: 3.5, label: "Thick" },
  ],
  pos: [
    { value: null, label: "Default" },
    { value: 100, label: "Bottom" },
    { value: 92, label: "Raised" },
    { value: 82, label: "Higher" },
  ],
  // sub-font-size, in mpv's scaled pixels (55 by default); `scale` multiplies it
  fontSize: [
    { value: null, label: "Default" },
    { value: 44, label: "44" },
    { value: 55, label: "55" },
    { value: 66, label: "66" },
    { value: 80, label: "80" },
  ],
  alignX: [
    { value: null, label: "Default" },
    { value: "left", label: "Left" },
    { value: "center", label: "Center" },
    { value: "right", label: "Right" },
  ],
  alignY: [
    { value: null, label: "Default" },
    { value: "top", label: "Top" },
    { value: "center", label: "Middle" },
    { value: "bottom", label: "Bottom" },
  ],
} as const;

export type Shadow = "default" | "none" | "soft" | "strong";

/** The shadow behind the text: its offset and colour together. */
export function shadowOf(style: SubtitleStyle): Shadow {
  if (style.shadowOffset == null && style.shadowColor == null) return "default";
  if (style.shadowOffset === 0) return "none";
  return (style.shadowOffset ?? 0) >= 2.5 ? "strong" : "soft";
}

/** The patch for a shadow choice. mpv's shadow colour is its box colour too
 * (`sub-shadow-color` is another name for `sub-back-color`), so with the Box background
 * only the offset changes and the box keeps its colour. */
export function shadowPatch(shadow: Shadow, style: SubtitleStyle): Partial<SubtitleStyle> {
  const box = backgroundOf(style) === "box";
  const color = (c: string) => (box ? {} : { shadowColor: c });
  switch (shadow) {
    case "default":
      return { shadowOffset: null, shadowColor: null };
    case "none":
      return { shadowOffset: 0, shadowColor: null };
    case "soft":
      return { shadowOffset: 1.5, ...color("#80000000") };
    case "strong":
      return { shadowOffset: 2.5, ...color("#FF000000") };
  }
}

export type Background = "default" | "outline" | "box";

export function backgroundOf(style: SubtitleStyle): Background {
  if (style.borderStyle === "background-box" || style.borderStyle === "opaque-box") return "box";
  if (style.borderStyle === "outline-and-shadow") return "outline";
  return "default";
}

export function backgroundPatch(background: Background): Partial<SubtitleStyle> {
  switch (background) {
    case "default":
      return { borderStyle: null, backColor: null };
    case "outline":
      return { borderStyle: "outline-and-shadow", backColor: null };
    case "box":
      // (the box colour is mpv's shadow colour too: a shadow colour would replace it)
      return { borderStyle: "background-box", backColor: BOX_COLOR, shadowColor: null };
  }
}

/** Every field unset: mpv's own look (or external mpv's config). */
export const NO_STYLE: SubtitleStyle = {
  scale: null,
  fontSize: null,
  font: null,
  bold: null,
  color: null,
  outlineColor: null,
  outlineSize: null,
  borderStyle: null,
  backColor: null,
  shadowOffset: null,
  shadowColor: null,
  pos: null,
  marginY: null,
  alignX: null,
  alignY: null,
  assOverride: null,
};

/** An mpv colour (#RRGGBB or #AARRGGBB) as CSS. */
export function cssColor(mpv: string | null, fallback: string): string {
  const hex = mpv?.replace(/^#/, "");
  if (!hex || !/^([0-9a-f]{6}|[0-9a-f]{8})$/i.test(hex)) return fallback;
  if (hex.length === 6) return `#${hex}`;
  const alpha = parseInt(hex.slice(0, 2), 16) / 255;
  const rgb = [2, 4, 6].map((i) => parseInt(hex.slice(i, i + 2), 16));
  return `rgba(${rgb.join(", ")}, ${alpha.toFixed(2)})`;
}

/** The preview text's CSS for `style`, in a box of `height` pixels. */
export function previewStyle(style: SubtitleStyle, height: number): CSSProperties {
  // mpv's default text is 55 "scaled pixels" of a 720-pixel-high video, its outline 1.65
  const size = height * ((style.fontSize ?? 55) / 720) * (style.scale ?? 1);
  const outlineSize = style.outlineSize ?? 1.65;
  const outline = outlineSize > 0 ? Math.max(0.8, outlineSize * (height / 720) * 2) : 0;
  const outlineColor = cssColor(style.outlineColor, "#000000");
  const box = backgroundOf(style) === "box";
  const shadows = box
    ? "none"
    : [
        ...[-1, 0, 1].flatMap((x) => [-1, 0, 1].map((y) => [x, y])).filter(([x, y]) => x || y),
      ]
        .map(([x, y]) => `${(x * outline).toFixed(1)}px ${(y * outline).toFixed(1)}px 0 ${outlineColor}`)
        .concat(
          (style.shadowOffset ?? 0) > 0
            ? [
                `${((style.shadowOffset ?? 0) * (height / 720) * 2).toFixed(1)}px ${((style.shadowOffset ?? 0) * (height / 720) * 2).toFixed(1)}px 0 ${cssColor(style.shadowColor, "rgba(0,0,0,0.5)")}`,
              ]
            : [],
        )
        .join(", ");
  const alignY = style.alignY ?? "bottom";
  const alignX = style.alignX ?? "center";
  return {
    fontFamily: style.font ?? "sans-serif",
    fontWeight: style.bold ? 700 : 500,
    fontSize: `${size.toFixed(1)}px`,
    color: cssColor(style.color, "#FFFFFF"),
    textShadow: outline > 0 ? shadows : "none",
    background: box ? cssColor(style.backColor ?? BOX_COLOR, "rgba(0,0,0,0.6)") : "transparent",
    padding: box ? "0.1em 0.35em" : 0,
    textAlign: alignX,
    // sub-pos 100 is the bottom, lower values move the text up
    ...(alignY === "top"
      ? { top: "5%" }
      : alignY === "center"
        ? { top: "50%", transform: "translateY(-50%)" }
        : { bottom: `${Math.max(2, (100 - (style.pos ?? 100)) + 5)}%` }),
    ...(alignX === "left" ? { left: "4%" } : alignX === "right" ? { right: "4%" } : {}),
  };
}
