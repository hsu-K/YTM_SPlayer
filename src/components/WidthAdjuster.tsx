import type { Layout } from "../hooks/useLayout";

/**
 * Shown instead of the song info while adjusting the width: a slider in the
 * text area, and the current value + a done button in the controls area.
 * The slider has a fixed width so it doesn't move under the cursor while the
 * bar resizes.
 */
export function WidthAdjuster({ layout }: { layout: Layout }) {
  return (
    <>
      <div className="meta adjust">
        <input
          type="range"
          className="width-slider"
          min={layout.min}
          max={layout.max}
          step={1}
          value={layout.width}
          onChange={(e) => layout.preview(Number(e.currentTarget.value))}
          aria-label="寬度"
          autoFocus
        />
      </div>
      <div className="controls-wrap">
        <div className="adjust-actions">
          <span className="width-value">{layout.width}px</span>
          <button className="done" onClick={layout.finish} title="完成 (Enter / Esc)">
            <svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden>
              <path d="M9 16.2 4.8 12l-1.4 1.4L9 19 21 7l-1.4-1.4z" />
            </svg>
          </button>
        </div>
      </div>
    </>
  );
}
