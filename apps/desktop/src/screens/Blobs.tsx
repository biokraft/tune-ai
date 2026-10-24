/** Three soft colour fields behind the glass. */
export function Blobs() {
  return (
    <div aria-hidden="true" className="pointer-events-none absolute inset-0 -z-10 overflow-hidden">
      <div
        aria-hidden="true"
        className="blob"
        style={{
          left: "-10%",
          top: "45%",
          width: 620,
          height: 520,
          background: "rgb(var(--kick))",
        }}
      />
      <div
        aria-hidden="true"
        className="blob"
        style={{ left: "36%", top: -220, width: 720, height: 460, background: "rgb(var(--ride))" }}
      />
      <div
        aria-hidden="true"
        className="blob"
        style={{ left: "75%", top: "62%", width: 560, height: 460, background: "rgb(var(--acid))" }}
      />
    </div>
  );
}
