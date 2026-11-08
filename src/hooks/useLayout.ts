import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface LayoutInfo {
  width: number;
  min: number;
  max: number;
  adjusting: boolean;
}

export interface Layout {
  width: number;
  min: number;
  max: number;
  /** Width-adjust mode (entered from the right-click menu). */
  adjusting: boolean;
  /** Live preview while dragging the slider. */
  preview: (width: number) => void;
  /** Save the current width and leave adjust mode. */
  finish: () => void;
}

const applyCssWidth = (w: number) =>
  document.documentElement.style.setProperty("--bar-collapsed", `${w}px`);

/** Collapsed bar width (persisted by Rust, see layout.rs) and adjust mode. */
export function useLayout(): Layout {
  const [info, setInfo] = useState<LayoutInfo>({
    width: 220,
    min: 140,
    max: 360,
    adjusting: false,
  });
  const widthRef = useRef(info.width);
  const frame = useRef<number | null>(null);

  useEffect(() => {
    invoke<LayoutInfo>("get_layout")
      .then((l) => {
        widthRef.current = l.width;
        applyCssWidth(l.width);
        setInfo(l);
      })
      .catch(console.error);
    const unlisten = listen<boolean>("adjust-width", (e) =>
      setInfo((s) => ({ ...s, adjusting: e.payload })),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const preview = useCallback((width: number) => {
    widthRef.current = width;
    applyCssWidth(width);
    setInfo((s) => ({ ...s, width }));
    // resize the OS window at most once per frame while dragging
    if (frame.current === null) {
      frame.current = requestAnimationFrame(() => {
        frame.current = null;
        invoke("set_bar_width", { width: widthRef.current, persist: false }).catch(
          console.error,
        );
      });
    }
  }, []);

  const finish = useCallback(() => {
    invoke<number>("set_bar_width", { width: widthRef.current, persist: true })
      .then((w) => {
        widthRef.current = w;
        applyCssWidth(w);
        setInfo((s) => ({ ...s, width: w }));
      })
      .catch(console.error);
    invoke("set_adjusting", { adjusting: false }).catch(console.error);
  }, []);

  // Esc / Enter also finish adjusting
  useEffect(() => {
    if (!info.adjusting) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Enter") finish();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [info.adjusting, finish]);

  return { ...info, preview, finish };
}
