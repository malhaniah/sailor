import type { ProfileView } from "../../lib/types";

/** Profile colours (Rust `profiles::COLORS`), as oklch hues. */
export const PROFILE_COLORS: Record<string, string> = {
  slate: "oklch(0.55 0.02 260)",
  red: "oklch(0.62 0.17 25)",
  orange: "oklch(0.7 0.15 55)",
  amber: "oklch(0.78 0.14 80)",
  green: "oklch(0.68 0.14 150)",
  teal: "oklch(0.68 0.1 195)",
  blue: "oklch(0.62 0.14 255)",
  violet: "oklch(0.6 0.16 300)",
};

export const COLOR_IDS = Object.keys(PROFILE_COLORS);

/** A profile's picture, or its initial on its colour. */
export function ProfileAvatar({ profile, size = 40 }: { profile: Pick<ProfileView, "name" | "color" | "avatar">; size?: number }) {
  const ring = PROFILE_COLORS[profile.color] ?? PROFILE_COLORS.slate;
  const style = {
    width: size,
    height: size,
    borderRadius: Math.round(size * 0.28),
    boxShadow: `0 0 0 2px ${ring}`,
  };
  if (profile.avatar) {
    return <img src={profile.avatar} alt="" style={{ ...style, objectFit: "cover" }} />;
  }
  return (
    <div
      className="flex items-center justify-center font-semibold text-canvas"
      style={{ ...style, background: ring, fontSize: size * 0.42 }}
    >
      {profile.name.trim().slice(0, 1).toUpperCase() || "?"}
    </div>
  );
}

/** Resize a picked image to at most `max` px on its longer side and encode it as WebP
 * (falling back to PNG), so avatars stay small. Adapted from Harbor's avatar-utils.ts. */
export async function resizeImage(file: File, max = 256): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const image = new Image();
      image.onload = () => resolve(image);
      image.onerror = () => reject(new Error("That file is not a picture Zeiler can read."));
      image.src = url;
    });
    const scale = Math.min(1, max / Math.max(img.width, img.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(img.width * scale));
    canvas.height = Math.max(1, Math.round(img.height * scale));
    canvas.getContext("2d")?.drawImage(img, 0, 0, canvas.width, canvas.height);
    const webp = canvas.toDataURL("image/webp", 0.85);
    return webp.startsWith("data:image/webp") ? webp : canvas.toDataURL("image/png");
  } finally {
    URL.revokeObjectURL(url);
  }
}
