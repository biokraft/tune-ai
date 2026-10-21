import { useSyncExternalStore } from "react";
import type { StudioStore } from "./store/studioStore";

export default function App({ store }: { store: StudioStore }) {
  const view = useSyncExternalStore(store.subscribe, store.get);
  if (view.connection === "lost") return <p role="alert">Lost connection to the studio.</p>;
  return <p>{view.state?.song?.name ?? "Connecting to the studio…"}</p>;
}
