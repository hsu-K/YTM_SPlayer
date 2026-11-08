import { invoke } from "@tauri-apps/api/core";
import type { ControlAction, NowPlaying } from "../types";

function send(action: ControlAction) {
  invoke("media_control", { action }).catch(console.error);
}

const Icon = ({ d }: { d: string }) => (
  <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden>
    <path d={d} />
  </svg>
);

const PREV = "M6 6h2v12H6zm3.5 6 8.5 6V6z";
const NEXT = "M16 6h2v12h-2zM6 18l8.5-6L6 6z";
const PLAY = "M8 5v14l11-7z";
const PAUSE = "M6 5h4v14H6zm8 0h4v14h-4z";

export function Controls({ np }: { np: NowPlaying }) {
  const playing = np.status === "playing";
  return (
    <div className="controls">
      <button onClick={() => send("previous")} disabled={!np.canPrev} title="上一首">
        <Icon d={PREV} />
      </button>
      <button
        className="primary"
        onClick={() => send("playPause")}
        disabled={!np.canPlayPause}
        title={playing ? "暫停" : "播放"}
      >
        <Icon d={playing ? PAUSE : PLAY} />
      </button>
      <button onClick={() => send("next")} disabled={!np.canNext} title="下一首">
        <Icon d={NEXT} />
      </button>
    </div>
  );
}
