import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

const INTERACTIVE = "button, select, input, textarea, a, [data-no-drag]";

/** Frameless window: left-dragging anywhere except controls moves the window. */
export function useWindowDrag() {
  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      if (e.button !== 0 || e.detail > 1) return; // left button, ignore double-click
      if ((e.target as Element | null)?.closest(INTERACTIVE)) return;
      e.preventDefault(); // avoid text selection while dragging
      getCurrentWindow().startDragging().catch(console.error);
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, []);
}
