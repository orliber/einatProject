// The recordings she keeps for herself with a session note (D-208): one she records here after
// the meeting, or a file she recorded on her phone. Like the personal note they are hers
// alone: sealed in the vault with the clinic's records, never printed, exported, copied into a
// case or sent anywhere. The sound is read from the vault only when she plays it, and the
// player lives only while the dialog is open.
import { useEffect, useRef, useState } from "react";
import { useApp } from "../../App";
import { clinicApi } from "../../ipc/clinic";
import type { RecordingInfo } from "../../ipc/generated/RecordingInfo";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import { useDeleteRecording, useSaveRecording } from "./clinicData";
import { formatDate } from "./format";
import { ConfirmDialog } from "./parts";

/** The core's limits: the largest sound, and how many one note keeps. */
export const RECORDING_MAX_BYTES = 40 * 1024 * 1024;
export const RECORDINGS_MAX = 10;
/** A recording made here stops by itself after two hours (well under the size limit at the rate below). */
const LONGEST_SECONDS = 2 * 60 * 60;
/** Speech, clearly: about 14 MB an hour. */
const BITS_PER_SECOND = 32_000;

/** "12:05" or "1:02:05". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** "3.4 מגה-בייט" or "820 קילו-בייט". */
export function sizeWords(bytes: number): string {
  return bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(1)} מגה-בייט` : `${Math.max(1, Math.round(bytes / 1024))} קילו-בייט`;
}

/** The sound as base64, without the `data:` prefix. */
function base64Of(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result ?? "");
      resolve(url.slice(url.indexOf(",") + 1));
    };
    reader.onerror = () => reject(new Error("read"));
    reader.readAsDataURL(blob);
  });
}

function blobOf(mime: string, data: string): Blob {
  const raw = atob(data);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return new Blob([bytes], { type: mime });
}

/** The kind of sound this computer records, by what it supports. */
function recorderMime(): string | undefined {
  if (typeof MediaRecorder === "undefined") return undefined;
  return ["audio/webm;codecs=opus", "audio/webm", "audio/mp4", "audio/ogg;codecs=opus"].find((m) => MediaRecorder.isTypeSupported(m));
}

/** How long a file is, when the window can tell; 0 otherwise. */
function durationOf(blob: Blob): Promise<number> {
  return new Promise((resolve) => {
    const url = URL.createObjectURL(blob);
    const audio = new Audio();
    const done = (seconds: number) => {
      URL.revokeObjectURL(url);
      resolve(Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds) : 0);
    };
    audio.preload = "metadata";
    audio.onloadedmetadata = () => done(audio.duration);
    audio.onerror = () => done(0);
    setTimeout(() => done(0), 3000);
    audio.src = url;
  });
}

/** Why the microphone could not be used, in her words. */
function micProblem(e: unknown): string {
  const name = (e as { name?: string }).name ?? "";
  if (name === "NotAllowedError" || name === "SecurityError") return "אין לתוכנה רשות להשתמש במיקרופון. אפשר לאשר את המיקרופון בהגדרות המחשב ולנסות שוב, או להקליט בטלפון ולצרף את הקובץ.";
  if (name === "NotFoundError" || name === "OverconstrainedError") return "לא נמצא מיקרופון במחשב. אפשר לחבר מיקרופון, או להקליט בטלפון ולצרף את הקובץ.";
  if (name === "NotReadableError") return "המיקרופון תפוס בתוכנה אחרת (למשל שיחת וידאו). אפשר לסגור אותה ולנסות שוב.";
  return "ההקלטה לא התחילה. אפשר לנסות שוב, או להקליט בטלפון ולצרף את הקובץ.";
}

type Phase = { kind: "idle" } | { kind: "recording"; seconds: number } | { kind: "saving" };

export function Recordings({ note, patientId, visitId, date, onNote, onBusy }: {
  /** The note as kept; `null` before it was ever saved (the first recording creates it). */
  note: SessionNote | null;
  patientId: string;
  visitId: string | null;
  date: string;
  onNote: (note: SessionNote) => void;
  /** True while a recording is under way or being saved: closing would lose it. */
  onBusy: (busy: boolean) => void;
}) {
  const { notify } = useApp();
  const save = useSaveRecording();
  const remove = useDeleteRecording();
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const [playing, setPlaying] = useState<{ id: string; url: string } | null>(null);
  const [loading, setLoading] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<RecordingInfo | null>(null);
  const recorder = useRef<MediaRecorder | null>(null);
  const stream = useRef<MediaStream | null>(null);
  const chunks = useRef<Blob[]>([]);
  const timer = useRef<number | null>(null);
  const started = useRef(0);
  const file = useRef<HTMLInputElement>(null);
  const list = note?.recordings ?? [];
  const full = list.length >= RECORDINGS_MAX;
  const busy = phase.kind !== "idle";

  useEffect(() => onBusy(busy), [busy, onBusy]);

  // Closed while recording: the microphone is let go and nothing is kept. The player's sound is let go too.
  useEffect(() => () => {
    if (timer.current !== null) window.clearInterval(timer.current);
    if (recorder.current && recorder.current.state !== "inactive") {
      recorder.current.ondataavailable = null;
      recorder.current.onstop = null;
      recorder.current.stop();
    }
    stream.current?.getTracks().forEach((t) => t.stop());
  }, []);
  useEffect(() => () => {
    if (playing) URL.revokeObjectURL(playing.url);
  }, [playing]);

  async function keep(blob: Blob, mime: string, seconds: number, name: string) {
    if (blob.size === 0) {
      setError("ההקלטה ריקה. אפשר לנסות שוב.");
      return;
    }
    if (blob.size > RECORDING_MAX_BYTES) {
      setError(`ההקלטה גדולה מדי. אפשר לשמור הקלטה של עד ${RECORDING_MAX_BYTES / (1024 * 1024)} מגה-בייט.`);
      return;
    }
    setPhase({ kind: "saving" });
    try {
      const data = await base64Of(blob);
      const saved = await save.mutateAsync({ patientId, noteId: note?.id ?? "", visitId, date, mime, seconds, name, data });
      onNote(saved);
      notify("ההקלטה נשמרה בכספת");
    } catch (e) {
      setError((e as { message?: string }).message ?? "ההקלטה לא נשמרה. אפשר לנסות שוב.");
    } finally {
      setPhase({ kind: "idle" });
    }
  }

  async function start() {
    setError(null);
    const mime = recorderMime();
    if (!navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
      setError("המחשב הזה לא מאפשר להקליט מתוך התוכנה. אפשר להקליט בטלפון ולצרף את הקובץ.");
      return;
    }
    try {
      const mic = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.current = mic;
      const rec = new MediaRecorder(mic, { ...(mime ? { mimeType: mime } : {}), audioBitsPerSecond: BITS_PER_SECOND });
      recorder.current = rec;
      chunks.current = [];
      rec.ondataavailable = (e) => {
        if (e.data.size > 0) chunks.current.push(e.data);
      };
      rec.onstop = () => {
        mic.getTracks().forEach((t) => t.stop());
        stream.current = null;
        if (timer.current !== null) window.clearInterval(timer.current);
        timer.current = null;
        const type = rec.mimeType || mime || "audio/webm";
        const seconds = Math.round((Date.now() - started.current) / 1000);
        void keep(new Blob(chunks.current, { type }), type, seconds, "");
        chunks.current = [];
      };
      started.current = Date.now();
      rec.start(1000);
      setPhase({ kind: "recording", seconds: 0 });
      timer.current = window.setInterval(() => {
        const seconds = Math.floor((Date.now() - started.current) / 1000);
        setPhase({ kind: "recording", seconds });
        if (seconds >= LONGEST_SECONDS && rec.state === "recording") rec.stop();
      }, 1000);
    } catch (e) {
      stream.current?.getTracks().forEach((t) => t.stop());
      stream.current = null;
      setError(micProblem(e));
    }
  }

  function stop() {
    if (recorder.current && recorder.current.state === "recording") recorder.current.stop();
  }

  /** Stops and keeps nothing. */
  function discard() {
    const rec = recorder.current;
    if (rec && rec.state !== "inactive") {
      rec.ondataavailable = null;
      rec.onstop = null;
      rec.stop();
    }
    stream.current?.getTracks().forEach((t) => t.stop());
    stream.current = null;
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    chunks.current = [];
    setPhase({ kind: "idle" });
  }

  async function attach(picked: File | undefined) {
    if (!picked) return;
    setError(null);
    const type = picked.type !== "" ? picked.type : /\.m4a$/i.test(picked.name) ? "audio/x-m4a" : /\.mp3$/i.test(picked.name) ? "audio/mpeg" : /\.wav$/i.test(picked.name) ? "audio/wav" : "";
    if (!type.startsWith("audio/")) {
      setError("אפשר לצרף רק קובץ שמע (m4a,‏ mp3,‏ wav,‏ ogg,‏ webm).");
      return;
    }
    await keep(picked, type, await durationOf(picked), picked.name);
  }

  async function play(r: RecordingInfo) {
    setError(null);
    setLoading(r.id);
    try {
      const sound = await clinicApi.recording(r.id);
      setPlaying({ id: r.id, url: URL.createObjectURL(blobOf(sound.mime, sound.data)) });
    } catch (e) {
      setError((e as { message?: string }).message ?? "ההקלטה לא נפתחה. אפשר לנסות שוב.");
    } finally {
      setLoading(null);
    }
  }

  return (
    <div className="stack clinic-recordings">
      <h4 className="clinic-recordings-title">הקלטות לעצמך</h4>
      {list.length > 0 && (
        <ul className="clinic-recordings-list">
          {list.map((r, i) => (
            <li key={r.id} className="stack">
              <span className="row">
                <span className="grow">
                  {r.name !== "" ? r.name : `הקלטה ${i + 1}`}
                  <span className="muted small"> · {formatDate(r.date)}{r.seconds > 0 ? <> · <span className="num" dir="ltr">{clock(r.seconds)}</span></> : null} · {sizeWords(r.bytes)}</span>
                </span>
                {playing?.id !== r.id && (
                  <button type="button" className="btn btn-small" disabled={loading !== null} onClick={() => void play(r)}>
                    {loading === r.id ? "פותחת…" : "להשמיע"}
                  </button>
                )}
                <button type="button" className="btn btn-small btn-quiet" disabled={busy} aria-label={`מחיקת ${r.name !== "" ? r.name : `הקלטה ${i + 1}`}`}
                  onClick={() => setConfirm(r)}>מחיקה</button>
              </span>
              {playing?.id === r.id && (
                // Sound only, from the vault, for as long as the dialog is open.
                <audio className="clinic-recording-player" controls autoPlay src={playing.url} aria-label={`השמעת ${r.name !== "" ? r.name : `הקלטה ${i + 1}`}`} />
              )}
            </li>
          ))}
        </ul>
      )}

      {phase.kind === "recording" ? (
        <div className="row clinic-recording-now" role="status">
          <span className="clinic-recording-dot" aria-hidden="true" />
          <span>מקליטה <span className="num" dir="ltr">{clock(phase.seconds)}</span></span>
          <span className="grow" />
          <button type="button" className="btn btn-small btn-primary" onClick={stop}>לעצור ולשמור</button>
          <button type="button" className="btn btn-small" onClick={discard}>ביטול</button>
        </div>
      ) : (
        <div className="row">
          <button type="button" className="btn btn-small" disabled={busy || full} onClick={() => void start()}>
            {phase.kind === "saving" ? "שומרת…" : "להקליט"}
          </button>
          <button type="button" className="btn btn-small" disabled={busy || full} onClick={() => file.current?.click()}>לצרף קובץ הקלטה</button>
          <input ref={file} type="file" accept="audio/*,.m4a,.mp3,.wav,.ogg,.webm" hidden
            onChange={(e) => {
              const picked = e.target.files?.[0];
              e.target.value = "";
              void attach(picked);
            }} />
        </div>
      )}
      <p className="hint">ההקלטה נשמרת מוצפנת בכספת, רק אצלך. היא לא נשלחת לשום מקום, לא ל-AI ולא לתיק האבחון.</p>
      {full && <p className="hint">אפשר לשמור עד {RECORDINGS_MAX} הקלטות בסיכום אחד.</p>}
      {error && <p className="note-warn" role="alert"><span>{error}</span></p>}

      {confirm && note && (
        <ConfirmDialog title="מחיקת ההקלטה" action="מחיקה" danger onClose={() => setConfirm(null)}
          onConfirm={async () => {
            const saved = await remove.mutateAsync({ noteId: note.id, id: confirm.id });
            if (playing?.id === confirm.id) setPlaying(null);
            onNote(saved);
            notify("ההקלטה נמחקה");
            setConfirm(null);
          }}>
          <p>ההקלטה תימחק מהכספת לגמרי, ואי אפשר יהיה לשחזר אותה.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
