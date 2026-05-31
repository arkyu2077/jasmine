import { useEffect, useRef, useState, type RefObject } from "react";
import { Play, Pause, Volume2, VolumeX } from "lucide-react";
import type { CanvasScene } from "../canvas/scene";

function fmt(t: number): string {
  const v = Number.isFinite(t) && t > 0 ? t : 0;
  const m = Math.floor(v / 60);
  const s = Math.floor(v % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** Playback bar for the single selected video. The scene owns the live
 *  HTMLVideoElement (rendered as a PixiJS texture); this bar drives it via the
 *  scene's video* methods. Position + scrub position refresh every frame from
 *  the scene (imperative, no React thrash); play/mute toggle React state only on
 *  actual change. Mirrors the SelectionBar overlay pattern. */
export function VideoControls({
  rootRef,
  getScene,
}: {
  rootRef: RefObject<HTMLDivElement | null>;
  getScene: () => CanvasScene | null;
}) {
  const rangeRef = useRef<HTMLInputElement>(null);
  const timeRef = useRef<HTMLSpanElement>(null);
  const seekingRef = useRef(false);
  const playingRef = useRef(false);
  const mutedRef = useRef(true);
  const [playing, setPlaying] = useState(false);
  const [muted, setMuted] = useState(true);

  useEffect(() => {
    let raf = 0;
    const tick = () => {
      raf = requestAnimationFrame(tick);
      const root = rootRef.current;
      if (!root) return;
      const scene = getScene();
      const st = scene?.videoControlState() ?? null;
      const anchor = scene?.videoControlAnchor() ?? null;
      if (!st || !anchor) {
        if (root.style.display !== "none") root.style.display = "none";
        return;
      }
      root.style.display = "flex";
      root.style.left = `${anchor.x}px`;
      root.style.top = `${anchor.y + 10}px`;
      if (st.playing !== playingRef.current) {
        playingRef.current = st.playing;
        setPlaying(st.playing);
      }
      if (st.muted !== mutedRef.current) {
        mutedRef.current = st.muted;
        setMuted(st.muted);
      }
      if (timeRef.current) timeRef.current.textContent = `${fmt(st.currentTime)} / ${fmt(st.duration)}`;
      if (rangeRef.current && !seekingRef.current) {
        const frac = st.duration > 0 ? st.currentTime / st.duration : 0;
        rangeRef.current.value = String(Math.round(frac * 1000));
      }
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [getScene, rootRef]);

  return (
    <div className="cm-vidbar" ref={rootRef} style={{ display: "none" }}>
      <button
        className="cm-vidbar__btn"
        title={playing ? "Pause" : "Play"}
        onClick={() => getScene()?.videoTogglePlay()}
      >
        {playing ? <Pause size={14} /> : <Play size={14} />}
      </button>
      <input
        ref={rangeRef}
        className="cm-vidbar__scrub"
        type="range"
        min={0}
        max={1000}
        defaultValue={0}
        onMouseDown={() => (seekingRef.current = true)}
        onMouseUp={() => (seekingRef.current = false)}
        onInput={(e) => getScene()?.videoSeekFrac(Number(e.currentTarget.value) / 1000)}
      />
      <span className="cm-vidbar__time" ref={timeRef}>
        0:00 / 0:00
      </span>
      <button
        className="cm-vidbar__btn"
        title={muted ? "Unmute" : "Mute"}
        onClick={() => getScene()?.videoToggleMuted()}
      >
        {muted ? <VolumeX size={14} /> : <Volume2 size={14} />}
      </button>
    </div>
  );
}
