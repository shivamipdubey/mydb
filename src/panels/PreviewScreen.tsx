import { useState } from "react";

import type { CommandOutcome } from "../api/backend";
import { RecordTable } from "./RecordTable";

/**
 * The preview screen (docs/05-confirmation-workflow.md step 5 onward).
 *
 * Shows the parsed intent in plain language, the records that will be
 * affected, and confirm, edit, and cancel. Raw query syntax is available but
 * collapsed: docs/12-ui-ux-guidelines.md requires plain language first, with
 * syntax as secondary detail, so a user can catch a misparse without reading
 * SQL.
 */
export function PreviewScreen({
  outcome,
  onConfirm,
  onEdit,
  onCancel,
  busy,
}: {
  outcome: Extract<CommandOutcome, { kind: "needsConfirmation" }>;
  onConfirm: () => void;
  onEdit: (text: string) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const { affected } = outcome;
  const count = affected.totalCount;

  return (
    <section className="panel preview-panel" aria-label="Preview">
      <header className="panel-header">
        <h2>Confirm this change</h2>
        {outcome.destructive && (
          // Not colour alone: docs/12 requires a label or icon alongside it.
          <span className="badge badge-destructive">
            <span aria-hidden="true">⚠</span> Destructive
          </span>
        )}
        {outcome.production && (
          <span className="badge badge-production">
            <span aria-hidden="true">●</span> Production
          </span>
        )}
      </header>

      <p className="intent">{outcome.description}</p>

      <p className={count === 0 ? "affected affected-none" : "affected"}>
        {count === 0
          ? "This matches no records. Nothing would change."
          : `${count} ${count === 1 ? "record" : "records"} will be affected.`}
      </p>

      {outcome.affectsEverything && count > 0 && (
        <p className="warning">
          <span aria-hidden="true">⚠</span> This has no filter. It affects every
          record in the table.
        </p>
      )}

      {affected.truncated && (
        <p className="note">
          Showing the first {affected.rows.length} of {count}. The count above is
          exact.
        </p>
      )}

      <RecordTable records={affected} />

      <details className="syntax">
        <summary>Show the query MYDB read this from</summary>
        <pre>{affected.statement}</pre>
      </details>

      {editing ? (
        <form
          className="edit-form"
          onSubmit={(event) => {
            event.preventDefault();
            if (draft.trim().length > 0) {
              onEdit(draft.trim());
            }
          }}
        >
          <label htmlFor="edit-command">Revise the command</label>
          <input
            id="edit-command"
            type="text"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="delete users where active is false"
            autoFocus
          />
          <div className="actions">
            <button type="submit" disabled={busy || draft.trim().length === 0}>
              Preview again
            </button>
            <button type="button" className="secondary" onClick={() => setEditing(false)}>
              Back
            </button>
          </div>
          <p className="note">
            A revised command is previewed again before anything runs.
          </p>
        </form>
      ) : (
        <div className="actions">
          <button
            type="button"
            className="confirm"
            onClick={onConfirm}
            disabled={busy}
          >
            Confirm
          </button>
          <button
            type="button"
            className="secondary"
            onClick={() => {
              setDraft(outcome.description);
              setEditing(true);
            }}
            disabled={busy}
          >
            Edit command
          </button>
          <button type="button" className="secondary" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
        </div>
      )}
    </section>
  );
}
