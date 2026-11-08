import "./App.css";
import { useMediaState } from "./hooks/useNowPlaying";
import { useAppContextMenu } from "./hooks/useContextMenu";
import { useWindowDrag } from "./hooks/useWindowDrag";
import { useBarHover } from "./hooks/useBarHover";
import { useLayout } from "./hooks/useLayout";
import { useCoverMetrics } from "./hooks/useCoverMetrics";
import { NowPlayingCard } from "./components/NowPlayingCard";
import { Controls } from "./components/Controls";
import { WidthAdjuster } from "./components/WidthAdjuster";

function App() {
  const state = useMediaState();
  useAppContextMenu();
  useWindowDrag();
  useCoverMetrics();
  const hovered = useBarHover();
  const layout = useLayout();
  const np = state?.nowPlaying ?? null;
  const expanded = layout.adjusting || (hovered && np !== null);

  const cover = (
    <div className="cover">
      {np?.thumbnail ? (
        <img src={np.thumbnail} alt="" draggable={false} />
      ) : (
        <div className="cover-placeholder">♪</div>
      )}
    </div>
  );

  let content;
  if (layout.adjusting) {
    content = (
      <>
        {cover}
        <WidthAdjuster layout={layout} />
      </>
    );
  } else if (np) {
    content = (
      <>
        <NowPlayingCard np={np} />
        {/* slides out to the right on a rightward swipe */}
        <div className="controls-wrap" aria-hidden={!expanded}>
          <Controls np={np} />
        </div>
      </>
    );
  } else {
    content = (
      <>
        {cover}
        <div className="meta">
          <div className="title">未偵測到音樂</div>
          <div className="artist">右鍵可切換來源</div>
        </div>
      </>
    );
  }

  return (
    <main
      className={`bar${expanded ? " expanded" : ""}${layout.adjusting ? " adjusting" : ""}`}
    >
      {content}
    </main>
  );
}

export default App;
