import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { createTauriClient } from "./tauriClient";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

/** A promise and its resolve, for driving IPC replies by hand. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

const settle = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

describe("createTauriClient", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    listenMock.mockReset();
  });

  it("a call waits until the event listener is registered", async () => {
    const registered = deferred<() => void>();
    listenMock.mockReturnValue(registered.promise);
    invokeMock.mockResolvedValue("state");
    const client = createTauriClient();
    client.subscribe(() => {});
    const result = client.call({ cmd: "get_state" });
    await settle();
    // An answer computed before the listener exists could miss the events after it.
    expect(invokeMock).not.toHaveBeenCalled();
    registered.resolve(() => {});
    await expect(result).resolves.toBe("state");
    expect(invokeMock).toHaveBeenCalledWith("studio", { command: { cmd: "get_state" } });
  });

  it("calls run one at a time, in order", async () => {
    const first = deferred<unknown>();
    invokeMock.mockReturnValueOnce(first.promise).mockResolvedValue(null);
    const client = createTauriClient();
    const drag = client.call({
      cmd: "set_param",
      args: { track: "acid", param: "acid303.cutoff", value: 900, commit: false },
    });
    const release = client.call({
      cmd: "set_param",
      args: { track: "acid", param: "acid303.cutoff", value: 900, commit: true },
    });
    await settle();
    // The release waits for the live value, so it can never overtake it.
    expect(invokeMock).toHaveBeenCalledTimes(1);
    first.resolve(null);
    await expect(release).resolves.toBeNull();
    await expect(drag).resolves.toBeNull();
    const commits = invokeMock.mock.calls.map(
      (c) => (c[1] as { command: { args: { commit: boolean } } }).command.args.commit,
    );
    expect(commits).toEqual([false, true]);
  });

  it("a failed call does not hold up the next", async () => {
    invokeMock.mockRejectedValueOnce("nothing to undo").mockResolvedValue(null);
    const client = createTauriClient();
    const undo = client.call({ cmd: "undo" });
    const stop = client.call({ cmd: "stop" });
    await expect(undo).rejects.toThrow("nothing to undo");
    await expect(stop).resolves.toBeNull();
  });

  it("a failed listen does not block calls", async () => {
    listenMock.mockRejectedValue(new Error("no event plugin"));
    invokeMock.mockResolvedValue(null);
    const client = createTauriClient();
    client.subscribe(() => {});
    await expect(client.call({ cmd: "stop" })).resolves.toBeNull();
  });
});
