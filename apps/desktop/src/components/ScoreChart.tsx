import type { Measure } from "../ipc/generated/Measure";
import type { ScoreScale } from "../ipc/generated/ScoreScale";

/** The scales drawn as a profile, with their mean and standard deviation. As in the Word
 *  export (D-049): index scores and subtest scaled scores only; a bar of a T score or a
 *  cut-off would suggest a comparison the scale does not support. */
export const PROFILE_SCALES: { scale: ScoreScale; title: string; mean: number; sd: number }[] = [
  { scale: "standard", title: "פרופיל המדדים", mean: 100, sd: 15 },
  { scale: "scaled", title: "פרופיל תת-המבחנים", mean: 10, sd: 3 },
];

const W = 360;
const LABEL_W = 132;
const VALUE_W = 34;
const ROW_H = 19;
const TOP = 4;
const AXIS_H = 18;
const PLOT_LEFT = 8;
const PLOT_RIGHT = W - LABEL_W - VALUE_W;

/**
 * A horizontal bar profile, drawn in code from the scores on the screen (round 54, D-094).
 * Right to left like the page: the name on the right, the score beside it, and the bar
 * growing leftwards from the lowest possible score. The background is the measure's own table
 * (the same bands as the words beside each score), with a dashed line at the mean.
 * Hebrew text is anchored with an explicit right-to-left direction and numbers with
 * left-to-right, so neither depends on the page around the drawing.
 */
export function ScoreChart(props: { title: string; mean: number; sd: number; rows: { measure: Measure; value: number }[] }) {
  const { rows, mean, sd } = props;
  const first = rows[0]?.measure;
  if (!first) return null;
  const lo = first.min - 0.5;
  const hi = first.max + 0.5;
  const x = (v: number) => PLOT_RIGHT - ((Math.min(Math.max(v, lo), hi) - lo) / (hi - lo)) * (PLOT_RIGHT - PLOT_LEFT);
  const plotH = rows.length * ROW_H;
  const H = TOP + plotH + AXIS_H;
  const ticks = [-2, -1, 0, 1, 2].map((k) => mean + k * sd).filter((t) => t >= first.min && t <= first.max);
  const bands = [...first.bands].sort((a, b) => a.min - b.min);
  const summary = rows.map((r) => `${r.measure.name_he} ${r.value}`).join(", ");

  return (
    <svg className="score-chart" viewBox={`0 0 ${W} ${H}`} width="100%" role="img" aria-label={`${props.title}: ${summary}`}>
      <g aria-hidden="true">
        {bands.map((b) => (
          <rect key={b.min} className={`chart-band chart-band-${b.level}`} x={x(b.max + 0.5)} y={TOP}
            width={Math.max(0, x(b.min - 0.5) - x(b.max + 0.5))} height={plotH} />
        ))}
        {bands.slice(1).map((b) => (
          <line key={b.min} className="chart-edge" x1={x(b.min - 0.5)} x2={x(b.min - 0.5)} y1={TOP} y2={TOP + plotH} />
        ))}
        <line className="chart-mean" x1={x(mean)} x2={x(mean)} y1={TOP} y2={TOP + plotH} />
        {ticks.map((t) => (
          <text key={t} className="chart-tick" x={x(t)} y={TOP + plotH + 13} direction="ltr" textAnchor="middle">{t}</text>
        ))}
      </g>
      {rows.map((r, i) => {
        const y = TOP + i * ROW_H;
        const bx = x(r.value);
        return (
          <g key={r.measure.key}>
            <title>{`${r.measure.name_he}: ${r.value}`}</title>
            <text className="chart-label" x={W - 2} y={y + ROW_H / 2 + 4} direction="rtl" textAnchor="start">{r.measure.name_he}</text>
            <text className="chart-value" x={W - LABEL_W - VALUE_W / 2} y={y + ROW_H / 2 + 4} direction="ltr" textAnchor="middle">{r.value}</text>
            <rect className="chart-bar" x={bx} y={y + 5} width={Math.max(2, PLOT_RIGHT - bx)} height={ROW_H - 10} rx={2} />
          </g>
        );
      })}
    </svg>
  );
}
