import { useEffect } from "react";

/** Nominal cover size (CSS px). */
const COVER = 40;
/** Bar border (CSS px); must match `.bar { border }` in App.css. */
const BORDER = 1;

/**
 * Keeps the gap around the cover identical on top, bottom and left, in
 * *device* pixels.
 *
 * At fractional scaling the numbers rarely divide evenly, e.g. at 125%:
 * window 52px = 65 device px, minus 2×1 border = 63, cover 40px = 50 →
 * 13 left over, which can't be split evenly (7 top / 6 bottom). So the cover
 * is nudged by one device pixel until the leftover is even, and the left
 * padding uses the same gap. Recomputed when the window moves to a monitor
 * with a different scale (that fires `resize`).
 */
export function useCoverMetrics() {
  useEffect(() => {
    const apply = () => {
      const dpr = window.devicePixelRatio || 1;
      const height = Math.round(window.innerHeight * dpr);
      const border = Math.max(1, Math.floor(BORDER * dpr)); // borders snap down to whole device px
      const inner = height - 2 * border;
      let cover = Math.round(COVER * dpr);
      if ((inner - cover) % 2 !== 0) cover -= 1;
      const gap = (inner - cover) / 2;

      const root = document.documentElement.style;
      root.setProperty("--cover-size", `${cover / dpr}px`);
      root.setProperty("--cover-gap", `${gap / dpr}px`);
    };
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, []);
}
