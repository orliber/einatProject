// Browser preview only: the score text, ported from crates/dv-domain/src/scores.rs.
// The tables themselves come from the core (instruments.json, checked by a Rust test).
import type { Instrument } from "../ipc/generated/Instrument";
import type { Measure } from "../ipc/generated/Measure";
import type { ScoreSheet } from "../ipc/generated/ScoreSheet";
import tables from "./instruments.json";

export const instruments = tables as Instrument[];

function fmt(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(1);
}

function normalCdf(z: number): number {
  const x = Math.abs(z) / Math.SQRT2;
  const t = 1 / (1 + 0.327_591_1 * x);
  const poly = t * (0.254_829_592 + t * (-0.284_496_736 + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
  const erf = 1 - poly * Math.exp(-x * x);
  return z >= 0 ? 0.5 * (1 + erf) : 0.5 * (1 - erf);
}

function percentile(m: Measure, v: number): string | null {
  const z = m.scale === "standard" ? (v - 100) / 15 : m.scale === "scaled" ? (v - 10) / 3 : null;
  if (z === null) return null;
  const p = normalCdf(z) * 100;
  if (p < 0.1) return "<0.1";
  if (p > 99.9) return ">99.9";
  if (p < 1 || p > 99) return fmt(Math.round(p * 10) / 10);
  return String(Math.round(p));
}

export function formatSheet(sheet: ScoreSheet): string {
  const inst = instruments.find((i) => i.key === sheet.instrument);
  if (!inst) throw new Error("כלי לא מוכר");
  let out = `${inst.name} – ${inst.description_he}\n`;
  if (sheet.module.trim()) out += `מודול: ${sheet.module.trim()}\n`;
  let group = "";
  const indexes: [string, number][] = [];
  let anyPercentile = false;
  for (const e of sheet.entries) {
    const m = inst.measures.find((x) => x.key === e.measure);
    if (!m) throw new Error(`מדד לא מוכר: ${e.measure}`);
    if (e.value < m.min || e.value > m.max) {
      throw new Error(`${m.name_he}: הציון ${fmt(e.value)} מחוץ לטווח האפשרי (${fmt(m.min)}–${fmt(m.max)})`);
    }
    if (m.group !== group) {
      group = m.group;
      out += `${group}:\n`;
    }
    const name = m.abbr ? `${m.name_he} (${m.abbr})` : m.name_he;
    let range = "";
    if (m.scale === "raw_cutoff") {
      if (m.key === "total" && sheet.cutoff !== null) {
        range = e.value >= sheet.cutoff ? `בנקודת החתך או מעליה (${fmt(sheet.cutoff)})` : `מתחת לנקודת החתך (${fmt(sheet.cutoff)})`;
      }
    } else {
      range = m.bands.find((b) => e.value >= b.min && e.value <= b.max)?.label ?? "";
    }
    const p = percentile(m, e.value);
    if (p) anyPercentile = true;
    const value = p ? `ציון ${fmt(e.value)}, אחוזון ${p}` : `ציון ${fmt(e.value)}`;
    const note = e.note.trim() ? `. ${e.note.trim()}` : "";
    out += range ? `- ${name}: ${value} – ${range}${note}\n` : `- ${name}: ${value}${note}\n`;
    if (m.scale === "standard" && m.group === "מדדים" && m.key !== "fsiq" && m.key !== "gac") indexes.push([m.name_he, e.value]);
  }
  if (indexes.length > 1) {
    const hi = indexes.reduce((a, b) => (b[1] > a[1] ? b : a));
    const lo = indexes.reduce((a, b) => (b[1] < a[1] ? b : a));
    if (hi[1] - lo[1] >= 15) {
      out += `פער של ${fmt(hi[1] - lo[1])} נקודות בין ${hi[0]} (${fmt(hi[1])}) לבין ${lo[0]} (${fmt(lo[1])}), סטיית תקן אחת או יותר. מובהקות הפער נבדקת לפי טבלאות המבחן.\n`;
    }
  }
  out += `${inst.scale_note_he}\n`;
  if (anyPercentile) out += "האחוזונים חושבו לפי העקומה הנורמלית.\n";
  out += "הטווחים חושבו בתוכנה לפי טבלה קבועה, לא על ידי מודל השפה.\n";
  if (sheet.notes.trim()) out += `הערות המאבחנת: ${sheet.notes.trim()}\n`;
  return out;
}
