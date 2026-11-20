// The small icon beside each part of Settings (D-205), drawn in the program's own line style
// (as the icons in components/ui.tsx), on a soft tile. Decoration only: the part's name says it.
import type { ReactNode } from "react";
import type { SettingsIconName } from "../screens/settingsPanes";

const PATHS: Record<SettingsIconName, ReactNode> = {
  // Sliders.
  general: <><path d="M4 7h10M18 7h2M4 17h4M12 17h8" /><circle cx="16" cy="7" r="2" /><circle cx="10" cy="17" r="2" /></>,
  // A spark: the AI.
  ai: <path d="M12 3c.5 4.5 2.5 6.5 7 7-4.5.5-6.5 2.5-7 7-.5-4.5-2.5-6.5-7-7 4.5-.5 6.5-2.5 7-7Z" />,
  // A shield.
  privacy: <><path d="M12 3 5 6v5c0 4.5 3 8 7 10 4-2 7-5.5 7-10V6Z" /><path d="m9 12 2 2 4-4" /></>,
  // A page.
  report: <><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" /><path d="M14 3v5h5M9 13h6M9 17h6" /></>,
  // A clipboard with a tick.
  tests: <><rect x="6" y="4" width="12" height="17" rx="2" /><path d="M9 4h6v3H9zM9 14l2 2 4-4" /></>,
  // A house: the clinic.
  clinic: <><path d="M4 11 12 4l8 7" /><path d="M6 10v10h12V10M12 13v4M10 15h4" /></>,
  // A box.
  backup: <><path d="M4 7h16v4H4zM6 11v9h12v-9" /><path d="M10 15h4" /></>,
  // A lock.
  security: <><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></>,
  // Arrows round: updates.
  about: <><path d="M20 12a8 8 0 0 1-14 5.3M4 12a8 8 0 0 1 14-5.3" /><path d="M18 3v4h-4M6 21v-4h4" /></>,
  // A calendar.
  calendar: <><rect x="4" y="5" width="16" height="16" rx="2" /><path d="M4 10h16M9 3v4M15 3v4" /></>,
  // A coin.
  money: <><circle cx="12" cy="12" r="8" /><path d="M14.5 9.5c-.5-1-1.5-1.5-2.5-1.5-1.5 0-2.5.8-2.5 2s1 1.6 2.5 2 2.5.8 2.5 2-1 2-2.5 2c-1 0-2-.5-2.5-1.5M12 6.5V8M12 16v1.5" /></>,
  // A path with its steps.
  path: <><circle cx="6" cy="18" r="2" /><circle cx="18" cy="6" r="2" /><path d="M8 18h6a3 3 0 0 0 0-6h-4a3 3 0 0 1 0-6h6" /></>,
  // A message.
  message: <path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" />,
};

export function SettingsIcon({ name }: { name: SettingsIconName }) {
  return (
    <span className="settings-icon" aria-hidden="true">
      <svg width={16} height={16} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
        {PATHS[name]}
      </svg>
    </span>
  );
}
