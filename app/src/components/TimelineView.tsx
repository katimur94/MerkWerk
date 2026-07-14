// Platzhalter-Komponente für die Timeline-Ansicht.
//
// TODO (spätere Task): app_sessions/events/snapshots über den Tauri-Command
// `list_timeline(from_ms, to_ms)` laden (siehe src-tauri/src/lib.rs sowie
// die Row-Structs in daemon/storage/src/model.rs, die die read-only
// SQLite-DB liefert) und als scrollbare Zeitleiste rendern: Fokuswechsel,
// Tipp-Bursts, Klick-Cluster und die zugehörigen Snapshot-Texte/-URLs.
export function TimelineView() {
  return (
    <section aria-label="Timeline">
      <h2>Timeline</h2>
      <p>Platzhalter — wird in einer späteren Task implementiert.</p>
    </section>
  );
}
