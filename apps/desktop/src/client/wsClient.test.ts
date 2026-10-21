import { FakeWebSocket } from "./fakeWebSocket";
import { createWsClient } from "./wsClient";

const Ws = FakeWebSocket as unknown as typeof WebSocket;

function connect() {
  const client = createWsClient("ws://127.0.0.1:4410/ws", Ws);
  const ws = FakeWebSocket.last!;
  return { client, ws };
}

describe("createWsClient", () => {
  it("call sends id, cmd and args, and resolves on its reply", async () => {
    const { client, ws } = connect();
    ws.open();
    const a = client.call({ cmd: "seek_bar", args: { bar: 8 } });
    const b = client.call({ cmd: "list_songs" });
    const [fa, fb] = ws.frames();
    expect(fa).toEqual({ id: fa!.id, cmd: "seek_bar", args: { bar: 8 } });
    expect(fb).toEqual({ id: fb!.id, cmd: "list_songs" });
    expect(fb!.id as number).toBeGreaterThan(fa!.id as number);
    // Replies may come in any order.
    ws.receive({ reply: fb!.id, ok: true, result: ["songs/mini/song.toml"] });
    ws.receive({ reply: fa!.id, ok: true, result: null });
    await expect(b).resolves.toEqual(["songs/mini/song.toml"]);
    await expect(a).resolves.toBeNull();
  });

  it("ok false rejects with the server error", async () => {
    const { client, ws } = connect();
    ws.open();
    const p = client.call({ cmd: "undo" });
    ws.receive({ reply: ws.frames()[0]!.id, ok: false, error: "nothing to undo" });
    await expect(p).rejects.toThrow("nothing to undo");
  });

  it("events reach subscribers", () => {
    const { client, ws } = connect();
    ws.open();
    const seen: unknown[] = [];
    const off = client.subscribe((e) => seen.push(e));
    const playhead = { event: "playhead", step: 37, bar: 2, beat: 1, playing: true };
    ws.receive(playhead);
    off();
    ws.receive({ event: "error", message: "late" });
    expect(seen).toEqual([playhead]);
  });

  it("close rejects pending calls", async () => {
    const { client, ws } = connect();
    ws.open();
    let lost = 0;
    client.onLost?.(() => (lost += 1));
    const p = client.call({ cmd: "stop" });
    ws.serverClose();
    await expect(p).rejects.toThrow("Lost connection to the studio");
    expect(lost).toBe(1);
    // A call after the close fails at once instead of hanging.
    await expect(client.call({ cmd: "stop" })).rejects.toThrow("Lost connection to the studio");
  });

  it("calls before open are queued", async () => {
    const { client, ws } = connect();
    const p = client.call({ cmd: "play", args: { from_bar: null } });
    expect(ws.sent).toEqual([]);
    ws.open();
    const [f] = ws.frames();
    expect(f).toMatchObject({ cmd: "play", args: { from_bar: null } });
    ws.receive({ reply: f!.id, ok: true, result: null });
    await expect(p).resolves.toBeNull();
  });
});
