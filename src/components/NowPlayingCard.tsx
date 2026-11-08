import type { NowPlaying } from "../types";
import { Marquee } from "./Marquee";

/**
 * Single-row: cover · title/artist. Controls are rendered by the parent.
 * The second line shows only the artist; hovering the info area shows a
 * tooltip with the full title and "Artist • Album • Year".
 */
export function NowPlayingCard({ np }: { np: NowPlaying }) {
  const title = np.title || "(無標題)";
  const full = np.subtitle || [np.artist, np.album].filter(Boolean).join(" • ");
  const artist = np.artist || full;

  return (
    <>
      <div className="cover">
        {np.thumbnail ? (
          <img src={np.thumbnail} alt="" draggable={false} />
        ) : (
          <div className="cover-placeholder">♪</div>
        )}
      </div>
      <div className="meta" title={`${title}\n${full}`}>
        <Marquee className="title" text={title} />
        {/* artist stays fixed; overflow is cut with "…" (no scrolling) */}
        <div className="artist">{artist}</div>
      </div>
    </>
  );
}
