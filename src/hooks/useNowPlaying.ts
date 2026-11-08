import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { MediaSnapshot } from "../types";

/** Subscribes to the `media-state` event emitted by the Rust SMTC worker. */
export function useMediaState(): MediaSnapshot | null {
  const [state, setState] = useState<MediaSnapshot | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlistenPromise = listen<MediaSnapshot>("media-state", (e) => {
      if (!disposed) setState(e.payload);
    });
    invoke<MediaSnapshot>("get_media_state")
      .then((s) => {
        if (!disposed) setState(s);
      })
      .catch(console.error);

    return () => {
      disposed = true;
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  return state;
}
