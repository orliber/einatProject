import { useEffect, useState } from "react";
import { ipc, type PingResponse } from "../ipc/client";
import { he } from "../i18n/he";

type State =
  | { kind: "checking" }
  | { kind: "ok"; info: PingResponse }
  | { kind: "error" };

export function CoreStatus() {
  const [state, setState] = useState<State>({ kind: "checking" });

  useEffect(() => {
    let alive = true;
    ipc
      .ping()
      .then((info) => alive && setState({ kind: "ok", info }))
      .catch(() => alive && setState({ kind: "error" }));
    return () => {
      alive = false;
    };
  }, []);

  const text =
    state.kind === "ok"
      ? `${he.core.connected} · v${state.info.core_version}`
      : state.kind === "error"
        ? he.core.disconnected
        : he.core.checking;

  return (
    <p role="status" className={`core-status core-status-${state.kind}`}>
      {text}
    </p>
  );
}
