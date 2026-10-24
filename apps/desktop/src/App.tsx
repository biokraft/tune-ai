import type { SongView, StudioState } from "./client/protocol";
import { FileErrorBanner, LostBanner, MessageBanner } from "./screens/Banners";
import { Blobs } from "./screens/Blobs";
import { OpenSongScreen } from "./screens/OpenSongScreen";
import { StudioScreen } from "./screens/StudioScreen";
import type { StudioStore } from "./store/studioStore";
import { sameButPlayhead, StudioStoreContext, useStudioSelector } from "./store/useStudio";

const hasSong = (s: StudioState | null): s is StudioState & { song: SongView } =>
  s !== null && s.path !== null && s.song !== null;

const everything = <T,>(v: T) => v;

export default function App({ store }: { store: StudioStore }) {
  // Playhead ticks do not re-render the app; the components that show the playhead read it.
  const view = useStudioSelector(store, everything, sameButPlayhead);
  const { state } = view;

  if (hasSong(state)) {
    return (
      <StudioStoreContext.Provider value={store}>
        <StudioScreen view={{ ...view, state }} store={store} />
      </StudioStoreContext.Provider>
    );
  }

  return (
    <div className="relative isolate flex h-screen flex-col gap-2 overflow-hidden bg-bg p-2 text-ink">
      <Blobs />
      {view.connection === "lost" && <LostBanner />}
      {view.message && <MessageBanner message={view.message} onDismiss={store.dismissMessage} />}
      {state === null ? (
        view.connection === "open" && (
          <p className="px-6 py-16 text-center text-ink2">Connecting to the studio…</p>
        )
      ) : state.path !== null ? (
        // A song is open but its file does not parse: say why, not which songs exist.
        <FileErrorBanner errors={state.file_errors} unopened />
      ) : (
        <OpenSongScreen send={store.send} />
      )}
    </div>
  );
}
