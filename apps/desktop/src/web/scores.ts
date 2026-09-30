// Browser preview only: the score text, ported from crates/dv-domain/src/scores.rs.
// The tables themselves come from the core (instruments.json, checked by a Rust test).
import type { Instrument } from "../ipc/generated/Instrument";
import type { Measure } from "../ipc/generated/Measure";
import type { ScoreSheet } from "../ipc/generated/ScoreSheet";
import type { ComparisonRow } from "../ipc/generated/ComparisonRow";
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

/** The same comparison as dv_domain::compare (D-029), for the browser preview. */
export function compareSheets(before: ScoreSheet[], after: ScoreSheet[]): ComparisonRow[] {
  const find = (s: ScoreSheet, key: string) => {
    const inst = instruments.find((i) => i.key === s.instrument);
    const m = inst?.measures.find((x) => x.key === key);
    return inst && m ? { inst, m } : null;
  };
  const band = (m: Measure, v: number) => m.bands.find((b) => v >= b.min && v <= b.max)?.label ?? "";
  const rows: ComparisonRow[] = [];
  for (const a of after) {
    for (const e of a.entries) {
      const now = find(a, e.measure);
      if (!now) continue;
      let prev: { inst: Instrument; m: Measure; v: number } | null = null;
      for (const b of before) {
        const f = find(b, e.measure);
        const be = b.entries.find((x) => x.measure === e.measure);
        if (f && be && f.m.scale === now.m.scale) { prev = { ...f, v: be.value }; break; }
      }
      if (!prev || rows.some((r) => r.abbr === now.m.abbr)) continue;
      const change = e.value - prev.v;
      const at = now.m.scale === "standard" || now.m.scale === "t_concern" ? 10 : now.m.scale === "scaled" ? 3 : Infinity;
      rows.push({
        measure: now.m.name_he, abbr: now.m.abbr, before_instrument: prev.inst.name, after_instrument: now.inst.name,
        before: prev.v, after: e.value, change, before_band: band(prev.m, prev.v), after_band: band(now.m, e.value), notable: Math.abs(change) >= at,
      });
    }
  }
  return rows;
}

export function comparisonText(rows: ComparisonRow[]): string {
  const lines = rows.map((r) => `${r.measure} (${r.abbr}): ${fmt(r.before)} ← ${fmt(r.after)} (שינוי ${r.change > 0 ? "+" : ""}${fmt(r.change)}${r.notable ? ", שינוי ששווה לבדוק" : ""}); טווח: ${r.before_band} ← ${r.after_band}${r.before_instrument === r.after_instrument ? "" : ` (${r.before_instrument} ← ${r.after_instrument})`}`);
  const careful = rows.some((r) => r.before_instrument !== r.after_instrument) ? "המבחנים שונים בין שני האבחונים, ולכן ההשוואה זהירה." : "";
  return ["השוואה לאבחון הקודם (מדדים שנמדדו בשני האבחונים):", ...lines, `הפרש קטן מ-10 נקודות בציון תקן (3 בציון מותאם) הוא לרוב בגבולות טעות המדידה. ${careful}`.trim()].join("\n");
}
