export type PlaybackStatus =
  | "playing"
  | "paused"
  | "stopped"
  | "changing"
  | "opened"
  | "closed";

export interface NowPlaying {
  sourceApp: string;
  title: string;
  artist: string;
  album: string;
  /** "Artist • Album • Year" from YouTube Music, "" if unavailable */
  subtitle: string;
  /** data URL (data:image/...;base64,...) */
  thumbnail: string | null;
  status: PlaybackStatus;
  positionMs: number;
  durationMs: number;
  /** Unix ms at which positionMs was valid */
  positionUpdatedAtMs: number;
  playbackRate: number;
  canPrev: boolean;
  canNext: boolean;
  canPlayPause: boolean;
}

export interface SourceInfo {
  /** SMTC AppUserModelId */
  id: string;
  name: string;
  isYtm: boolean;
  playing: boolean;
}

export interface MediaSnapshot {
  nowPlaying: NowPlaying | null;
  sources: SourceInfo[];
  /** null = auto */
  selectedSource: string | null;
  /** The manually selected source isn't running, so auto is used instead. */
  usingFallback: boolean;
}

export type ControlAction = "playPause" | "next" | "previous";
