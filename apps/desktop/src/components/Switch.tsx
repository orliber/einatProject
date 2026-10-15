// A switch and a row of choices for settings that save at once (round 50, D-087): no "save"
// button, the control itself is the whole setting. Both are buttons, so they read right in
// a right-to-left page and from the keyboard (Space or Enter flips a switch).
import type { ReactNode } from "react";
import "./Switch.css";

/** One setting as a row: its name, one line of explanation, and the switch at the end. */
export function Switch(props: {
  id: string;
  label: string;
  hint?: ReactNode;
  checked: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <div className="switch-row">
      <div className="switch-text">
        <label htmlFor={props.id} className="switch-label">{props.label}</label>
        {props.hint && <span className="hint">{props.hint}</span>}
      </div>
      <button id={props.id} type="button" role="switch" aria-checked={props.checked} disabled={props.disabled}
        className={props.checked ? "switch on" : "switch"} onClick={() => props.onChange(!props.checked)}>
        <span className="switch-knob" aria-hidden="true" />
      </button>
    </div>
  );
}

/** A few choices side by side, one pressed (minutes, sizes): a radio group of buttons. */
export function Segmented<T extends string | number>(props: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  disabled?: boolean;
  onChange: (value: T) => void;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={props.label}>
      {props.options.map((o) => (
        <button key={String(o.value)} type="button" role="radio" aria-checked={o.value === props.value} disabled={props.disabled}
          className={o.value === props.value ? "segment on" : "segment"} onClick={() => props.onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
