import { useSyncExternalStore } from "react";
import type { SongView, StudioState } from "./client/protocol";
import { LostBanner, MessageBanner } from "./screens/Banners";
import { OpenSongScreen } from "./screens/OpenSongScreen";
import { StudioScreen } from "./screens/StudioScreen";
import type { StudioStore } from "./store/studioStore";

const hasSong = (s: StudioState | null): s is StudioState & { song: SongView } =>
  s !== null && s.path !== null && s.song !== null;

export default function App({ store }: { store: StudioStore }) {
  const view = useSyncExternalStore(store.subscribe, store.get);
  const { state } = view;

  if (hasSong(state)) return <StudioScreen view={{ ...view, state }} store={store} />;

  return (
    <div className="flex min-h-screen flex-col bg-chassis text-silkscreen">
      {view.connection === "lost" && <LostBanner />}
      {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}
      {state === null ? (
        view.connection === "open" && (
          <p className="px-8 py-16 font-value text-sm text-silkscreen/70">
            Connecting to the studio…
          </p>
        )
      ) : (
        <OpenSongScreen send={store.send} />
      )}
    </div>
  );
}
