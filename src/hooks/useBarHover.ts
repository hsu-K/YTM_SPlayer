import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Whether the cursor is over the bar. Driven by the Rust hover thread
 * (hover.rs), which also makes the transparent part of the window
 * click-through — DOM mouseenter/leave is unreliable when that toggles.
 */
export function useBarHover(): boolean {
  const [hovered, setHovered] = useState(false);
  useEffect(() => {
    const unlisten = listen<boolean>("bar-hover", (e) => setHovered(e.payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  return hovered;
}
