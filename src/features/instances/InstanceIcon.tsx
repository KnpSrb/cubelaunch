import { useEffect, useState } from "react";

// Иконка инстанса по умолчанию: изометрический блок, цвет выбирается по имени.
// Пользовательские иконки появятся вместе с редактированием инстанса.

const PALETTE = [
  ["#6fd174", "#4caf50", "#2f7d33"],
  ["#7cc4ff", "#2e9bf0", "#1b6aa8"],
  ["#b9a6ff", "#7c5cff", "#5236c9"],
  ["#ffb27a", "#f2994a", "#b8682a"],
  ["#ff8a8d", "#e5484d", "#a52a2e"],
  ["#5fe0d0", "#14b8a6", "#0b7c70"],
  ["#f5d36b", "#e9b308", "#a17a00"],
];

function hash(s: string) {
  let h = 0;
  for (const c of s) h = (h * 31 + c.codePointAt(0)!) >>> 0;
  return h;
}

export function InstanceIcon({ name, size, src }: { name: string; size: number; src?: string | null }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [src]);
  if (src && !failed) {
    return (
      <img src={src} alt="" width={size} height={size} onError={() => setFailed(true)} className="shrink-0 rounded-md object-cover" style={{ width: size, height: size }} />
    );
  }
  const [top, left, right] = PALETTE[hash(name) % PALETTE.length];
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true">
      <polygon points="32,6 58,20 32,34 6,20" fill={top} />
      <polygon points="6,20 32,34 32,60 6,46" fill={left} />
      <polygon points="58,20 32,34 32,60 58,46" fill={right} />
    </svg>
  );
}
