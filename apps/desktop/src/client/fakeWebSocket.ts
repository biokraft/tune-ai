/** A minimal WebSocket stand-in for tests: records sends, lets the test drive open/message/close. */
export class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static last: FakeWebSocket | null = null;

  readonly url: string;
  readyState = FakeWebSocket.CONNECTING;
  sent: string[] = [];
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: ((e: CloseEvent) => void) | null = null;
  onerror: ((e: Event) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.last = this;
  }

  send(data: string) {
    if (this.readyState !== FakeWebSocket.OPEN) throw new Error("not open");
    this.sent.push(data);
  }

  close() {
    this.serverClose();
  }

  /** Test side: the connection opens. */
  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.(new Event("open"));
  }

  /** Test side: the server sends a frame. */
  receive(frame: unknown) {
    this.onmessage?.(new MessageEvent("message", { data: JSON.stringify(frame) }));
  }

  /** Test side: the connection ends. */
  serverClose() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.(new CloseEvent("close"));
  }

  /** The sent frames, parsed. */
  frames(): Record<string, unknown>[] {
    return this.sent.map((s) => JSON.parse(s) as Record<string, unknown>);
  }
}
