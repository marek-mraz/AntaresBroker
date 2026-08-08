import { useSyncExternalStore } from "react";
import { getVersion as boardVersion, subscribe as boardSubscribe } from "./state/board.js";
import { getVersion as transportVersion, transport } from "./broker/transport.js";

export const useBoard = () => useSyncExternalStore(boardSubscribe, boardVersion);
export const useTransport = () =>
  useSyncExternalStore((fn) => transport.subscribe(fn), transportVersion);
