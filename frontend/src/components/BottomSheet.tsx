import { ReactNode, useRef, useState } from "react";

export type SheetState = "peek" | "half" | "full";

const ORDER: SheetState[] = ["peek", "half", "full"];
const LABELS: Record<SheetState, string> = { peek: "réduit", half: "à mi-hauteur", full: "plein écran" };

interface Props {
  state: SheetState;
  onState: (s: SheetState) => void;
  label: string;
  children: ReactNode;
}

/** Hauteur visible de chaque position, en pixels (le panneau mesure 90 % de l'écran). */
function visibleHeight(s: SheetState): number {
  const h = window.innerHeight * 0.9;
  return s === "full" ? h : s === "half" ? window.innerHeight * 0.48 : 132;
}

/**
 * Panneau de recherche. Sur ordinateur : colonne à gauche de la carte.
 * Sur mobile : la carte occupe l'écran et ce panneau glisse par-dessus (poignée à tirer ou à toucher).
 */
export default function BottomSheet({ state, onState, label, children }: Props) {
  const drag = useRef<{ startY: number; startOffset: number; moved: boolean } | null>(null);
  const [dragOffset, setDragOffset] = useState<number | null>(null);

  const offsetOf = (s: SheetState) => window.innerHeight * 0.9 - visibleHeight(s);

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    drag.current = { startY: e.clientY, startOffset: offsetOf(state), moved: false };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dy = e.clientY - d.startY;
    if (Math.abs(dy) > 4) d.moved = true;
    if (d.moved) setDragOffset(Math.max(0, Math.min(offsetOf("peek"), d.startOffset + dy)));
  };
  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (!d.moved) {
      // simple toucher : on passe à la position suivante
      const i = ORDER.indexOf(state);
      onState(ORDER[(i + 1) % ORDER.length]);
    } else if (dragOffset != null) {
      // relâché : on se cale sur la position la plus proche
      const nearest = ORDER.reduce((best, s) =>
        Math.abs(offsetOf(s) - dragOffset) < Math.abs(offsetOf(best) - dragOffset) ? s : best,
      );
      onState(nearest);
    }
    setDragOffset(null);
  };

  return (
    <aside
      className={`panel sheet sheet-${state} ${dragOffset != null ? "dragging" : ""}`}
      style={dragOffset != null ? ({ "--sheet-offset": `${dragOffset}px` } as React.CSSProperties) : undefined}
    >
      <button
        className="sheet-handle"
        aria-label={`${label} : panneau ${LABELS[state]}. Toucher pour agrandir ou réduire.`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          drag.current = null;
          setDragOffset(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onState(ORDER[(ORDER.indexOf(state) + 1) % ORDER.length]);
          }
        }}
      >
        <span aria-hidden />
      </button>
      <div className="sheet-content">{children}</div>
    </aside>
  );
}
