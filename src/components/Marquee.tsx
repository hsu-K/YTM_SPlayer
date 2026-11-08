import { useLayoutEffect, useRef, useState } from "react";

/** Gap between the end of the text and its repeated copy (px). */
const GAP = 32;
/** Scroll speed (px/s). */
const SPEED = 30;

/**
 * Single-line text. When it doesn't fit, it scrolls left continuously:
 * the text is followed by a small gap and a copy of itself, looping forever.
 */
export function Marquee({ text, className }: { text: string; className?: string }) {
  const boxRef = useRef<HTMLDivElement>(null);
  const textRef = useRef<HTMLSpanElement>(null);
  const [textWidth, setTextWidth] = useState(0);
  const [overflowing, setOverflowing] = useState(false);

  useLayoutEffect(() => {
    const box = boxRef.current;
    const span = textRef.current;
    if (!box || !span) return;
    const measure = () => {
      const w = span.getBoundingClientRect().width;
      setTextWidth(w);
      setOverflowing(w > box.clientWidth + 1);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(box);
    return () => ro.disconnect();
  }, [text]);

  const shift = textWidth + GAP;

  return (
    <div ref={boxRef} className={`marquee ${overflowing ? "is-overflowing" : ""} ${className ?? ""}`}>
      <div
        key={text}
        className={overflowing ? "track scrolling" : "track"}
        style={
          overflowing
            ? ({
                "--shift": `-${shift}px`,
                animationDuration: `${shift / SPEED}s`,
              } as React.CSSProperties)
            : undefined
        }
      >
        <span ref={textRef}>{text}</span>
        {overflowing && (
          <span aria-hidden style={{ paddingLeft: GAP }}>
            {text}
          </span>
        )}
      </div>
    </div>
  );
}
