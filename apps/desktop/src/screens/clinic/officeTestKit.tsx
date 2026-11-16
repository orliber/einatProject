// What the tests of the follow-ups, the money and the settings tabs share: the clinic as the
// app shows it over the demo core (the route is the app's, and `go` changes it), a refusal
// for one request, and direct questions to the demo core. Every name is invented.
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import { dataOf, type ReplyData, type ReplyKind } from "../../ipc/clinic";
import type { ClinicReply } from "../../ipc/generated/ClinicReply";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import { appApi, Host, serve, type Override, type Served } from "../../test/host";
import { ClinicScreen } from "./ClinicScreen";

/** The Sunday morning the tests hold the clock at. */
export const NOW = { date: "2026-11-15", minute: 480 };

function Clinic({ first, onGo, notify }: { first: Route; onGo: (r: Route) => void; notify: (text: string) => void }) {
  const [route, setRoute] = useState(first);
  const app = appApi({ route, notify, go: (r) => { onGo(r); setRoute(r); } });
  return (
    <Host app={app}>
      {route.name === "clinic" ? <ClinicScreen tab={route.tab} patientId={route.patientId} /> : <p>מסך אחר: {route.name}</p>}
    </Host>
  );
}

/** The protected clipboard is the app's own; here it only says how long the copy stays. */
export const withClipboard: Override = (cmd) => (cmd === "copy_secret" ? 60 : undefined);

/** A refusal of the vault for one clinic request, in her words; everything else is answered as usual. */
export function refusing(op: ClinicRequest["op"], message: string): Override {
  return (cmd, args) => {
    if (cmd === "copy_secret") return 60;
    if (cmd === "clinic" && (args.request as ClinicRequest).op === op) throw { code: "refused", message, details: [], action: { kind: "dismiss" } };
    return undefined;
  };
}

/** One request held back until `release` is called (to see the page while it loads). */
export function holding(op: ClinicRequest["op"]): { over: Override; release: () => void } {
  let release: () => void = () => undefined;
  const wait = new Promise<void>((r) => (release = r));
  const over: Override = async (cmd, args, core) => {
    if (cmd === "copy_secret") return 60;
    if (cmd !== "clinic" || (args.request as ClinicRequest).op !== op) return undefined;
    await wait;
    return core.handle(cmd, args);
  };
  return { over, release: () => release() };
}

/** One question to the demo core, past the window. */
export async function ask<K extends ReplyKind>(served: Served, request: ClinicRequest, kind: K): Promise<ReplyData<K>> {
  return dataOf(request.op, (await served.core.handle("clinic", { request })) as ClinicReply, kind);
}

/** The id of a demo patient, by the child's name. */
export async function idOf(served: Served, name: string): Promise<string> {
  const id = (await ask(served, { op: "patients", today: NOW }, "patients")).find((r) => r.patient.name === name)?.patient.id;
  if (id === undefined) throw new Error(`no demo patient named ${name}`);
  return id;
}

/** Shows the clinic on `first` over a core that is already served (and, perhaps, prepared). */
export function show(served: Served, first: Route) {
  const user = userEvent.setup();
  const went: Route[] = [];
  const notes: string[] = [];
  const view = render(<Clinic first={first} onGo={(r) => went.push(r)} notify={(text) => notes.push(text)} />);
  const requests = () => served.calls.filter((c) => c.cmd === "clinic").map((c) => c.args.request as ClinicRequest);
  return { ...served, user, view, went, notes, requests };
}

/** Serves the demo core (after `prepare`, when the test needs more than the demo holds) and
 *  shows the clinic on `first`. */
export async function openOn(first: Route, over: Override = withClipboard, prepare?: (served: Served) => Promise<void>) {
  const served = await serve(over);
  await prepare?.(served);
  return show(served, first);
}
