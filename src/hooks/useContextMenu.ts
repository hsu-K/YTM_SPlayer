import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Replaces the WebView's default right-click menu with the app's native menu.
 * The menu is built, shown and handled entirely in Rust (see context_menu.rs);
 * showing it from JS via `Menu.popup()` could hang the app on Windows.
 */
export function useAppContextMenu() {
  useEffect(() => {
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      invoke("show_context_menu").catch(console.error);
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);
}
