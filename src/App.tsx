import { useCallback, useEffect, useState } from "react";

import {
  backend,
  errorMessage,
  type ActiveConnectionInfo,
  type CommandOutcome,
  type ConnectionSummary,
  type ExecutionSummary,
} from "./api/backend";
import { CommandBar } from "./panels/CommandBar";
import { ConnectionManager } from "./panels/ConnectionManager";
import { PreviewScreen } from "./panels/PreviewScreen";
import { RecordTable } from "./panels/RecordTable";

/**
 * Application shell.
 *
 * docs/12-ui-ux-guidelines.md requires the interface to be a set of
 * independent panels rather than one fixed layout, so later phases can add
 * the dashboard, audit log viewer, and recovery bin viewer without a
 * redesign. Phase 1 fills three panels: connections, command bar, and
 * preview.
 */
export function App() {
  const [connections, setConnections] = useState<ConnectionSummary[]>([]);
  const [active, setActive] = useState<ActiveConnectionInfo | null>(null);
  const [outcome, setOutcome] = useState<CommandOutcome | null>(null);
  const [executed, setExecuted] = useState<ExecutionSummary | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /// Bumped for every command result. Used as the preview screen's key, so a
  /// new preview is a new screen: after editing, the user sees the revised
  /// preview rather than being left in the edit form they just submitted.
  const [previewSeq, setPreviewSeq] = useState(0);

  const refresh = useCallback(() => {
    backend
      .listConnections()
      .then(setConnections)
      .catch((problem) => setError(errorMessage(problem)));
  }, []);

  useEffect(() => {
    refresh();
    backend.activeConnection().then(setActive).catch(() => undefined);
  }, [refresh]);

  /** Clears anything left from the previous command. */
  function reset() {
    setError(null);
    setNotice(null);
    setExecuted(null);
    setPreviewSeq((seq) => seq + 1);
  }

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch (problem) {
      setError(errorMessage(problem));
    } finally {
      setBusy(false);
    }
  }

  const pending = outcome?.kind === "needsConfirmation" ? outcome : null;

  return (
    <div className="app-shell">
      <header className="app-header">
        <h1>MYDB</h1>
        {active ? (
          <div className="active-connection">
            <span>
              Connected to <strong>{active.name}</strong>
            </span>
            {active.production && (
              // docs/12: a production connection must be visible at all times
              // while active, and never signalled by colour alone.
              <span className="badge badge-production">
                <span aria-hidden="true">●</span> Production
              </span>
            )}
          </div>
        ) : (
          <span className="muted">Not connected</span>
        )}
      </header>

      <div className="panels">
        <ConnectionManager
          connections={connections}
          active={active}
          onChanged={refresh}
          onConnected={(info) => {
            reset();
            setOutcome(null);
            setActive(info);
            setNotice(`Connected to ${info.name}.`);
            refresh();
          }}
          onError={setError}
        />

        <section className="panel" aria-label="Command entry">
          <header className="panel-header">
            <h2>Command</h2>
          </header>

          <CommandBar
            disabled={!active}
            busy={busy}
            placeholder={
              active
                ? "update users set active to false where id is 3"
                : "Connect to a database first"
            }
            onSubmit={(text) =>
              run(async () => {
                reset();
                setOutcome(await backend.submitCommand(text));
              })
            }
          />

          {active && active.tables.length > 0 && (
            <details className="schema">
              <summary>Tables on this connection</summary>
              <ul>
                {active.tables.map((table) => (
                  <li key={table.name}>
                    <strong>{table.name}</strong>
                    <span className="muted"> {table.columns.join(", ")}</span>
                  </li>
                ))}
              </ul>
            </details>
          )}

          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
          {notice && <p className="notice">{notice}</p>}

          {executed && (
            <div className="executed" role="status">
              <strong>Done.</strong> {executed.description} — {executed.rowsAffected}{" "}
              {executed.rowsAffected === 1 ? "record" : "records"} affected.
            </div>
          )}

          {executed?.historyWarning && (
            <p className="warning">
              <span aria-hidden="true">⚠</span> The change ran, but could not be
              recorded in the command history: {executed.historyWarning}
            </p>
          )}
        </section>

        {outcome?.kind === "readComplete" && (
          <section className="panel" aria-label="Result">
            <header className="panel-header">
              <h2>Result</h2>
            </header>
            <p className="intent">{outcome.description}</p>
            <p className="affected">
              {outcome.records.totalCount}{" "}
              {outcome.records.totalCount === 1 ? "record" : "records"} found.
            </p>
            <RecordTable records={outcome.records} />
            <details className="syntax">
              <summary>Show the query MYDB ran</summary>
              <pre>{outcome.records.statement}</pre>
            </details>
          </section>
        )}

        {pending && (
          <PreviewScreen
            key={previewSeq}
            outcome={pending}
            busy={busy}
            onConfirm={(authorization) =>
              run(async () => {
                const summary = await backend.confirmCommand(authorization);
                setOutcome(null);
                setExecuted(summary);
              })
            }
            onEdit={(text) =>
              run(async () => {
                reset();
                setOutcome(await backend.editCommand(text));
              })
            }
            onCancel={() =>
              run(async () => {
                const summary = await backend.cancelCommand();
                setOutcome(null);
                setNotice(`Cancelled: ${summary}. Nothing was changed.`);
              })
            }
          />
        )}
      </div>
    </div>
  );
}
