/**
 * Application shell.
 *
 * docs/12-ui-ux-guidelines.md requires the interface to be a set of independent
 * panels rather than one fixed layout, so later phases can add the dashboard,
 * audit log viewer, and recovery bin viewer without a redesign. The panel slots
 * below are that structure; phase 1 fills three of them (T9).
 */
export function App() {
  return (
    <main className="app-shell">
      <h1>MYDB</h1>
      <p className="scaffold-note">
        Phase 1 scaffold. Command bar, preview screen, and connection manager
        arrive in T9.
      </p>
    </main>
  );
}
