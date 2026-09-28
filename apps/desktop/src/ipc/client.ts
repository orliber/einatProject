// The only module that talks to the Rust core. Types come from Rust via ts-rs.
import { invoke } from "@tauri-apps/api/core";
import type { PingResponse } from "./generated/PingResponse";

export const ipc = {
  ping: (): Promise<PingResponse> => invoke<PingResponse>("ping"),
};

export type { PingResponse };
