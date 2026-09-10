import { useState } from "react";

import { extraStepSatisfied, type CommandOutcome } from "../api/backend";
import { RecordTable } from "./RecordTable";
import { TableOutline } from "./TableOutline";

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
  onConfirm: (authorization: string) => void;
  onEdit: (text: string) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [authorization, setAuthorization] = useState("");

  const { preview } = outcome;
  const isSchemaChange = preview.previewKind === "table";
  const count = isSchemaChange ? preview.rowCount : preview.totalCount;

  // A record being created does not yet exist, so "affected" reads wrongly.
  // Naming the operation is also a second chance for the user to notice a
  // misparse: an insert described as an update is worth catching here.
  const step = outcome.extraStep;
  // docs/12: the confirm button is disabled until any required extra step is
  // satisfied. The backend refuses regardless; this is so the user is not
  // invited to press something that will be rejected.
  const authorized = extraStepSatisfied(step, authorization);

  const verb =
    outcome.operation === "insert"
      ? "will be created"
      : outcome.operation === "update"
        ? "will be updated"
        : "will be deleted";

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
        {isSchemaChange
          ? count === 0
            ? "This table is already empty."
            : `${count} ${count === 1 ? "record" : "records"} will be deleted.`
          : count === 0
            ? "This matches no records. Nothing would change."
            : `${count} ${count === 1 ? "record" : "records"} ${verb}.`}
      </p>

      {outcome.operation === "drop_table" && (
        // A drop removes the structure too, which no record count conveys.
        // Someone who reads only the number would miss half of what goes.
        <p className="warning">
          <span aria-hidden="true">⚠</span> This removes the table itself, not
          just its records. Its structure, shown below, goes with it.
        </p>
      )}

      {outcome.affectsEverything && count > 0 && outcome.operation !== "insert" && (
        <p className="warning">
          <span aria-hidden="true">⚠</span> This has no filter. It affects every
          record in the table.
        </p>
      )}

      {preview.previewKind === "records" && preview.truncated && (
        <p className="note">
          Showing the first {preview.rows.length} of {count}. The count above is
          exact.
        </p>
      )}

      {preview.previewKind === "records" ? (
        <RecordTable records={preview} />
      ) : (
        <TableOutline table={preview} />
      )}

      <details className="syntax">
        <summary>Show the query MYDB read this from</summary>
        <pre>{preview.statement}</pre>
      </details>

      {step.kind !== "none" && (
        <div className="extra-step">
          <p className="warning">
            <span aria-hidden="true">●</span> {step.prompt}
          </p>
          <label htmlFor="authorization">
            {step.kind === "tableName"
              ? "Table name"
              : step.kind === "countOrConfirm"
                ? "Record count, or CONFIRM"
                : "Type CONFIRM"}
          </label>
          <input
            id="authorization"
            type="text"
            value={authorization}
            onChange={(event) => setAuthorization(event.target.value)}
            autoComplete="off"
          />
        </div>
      )}

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
            onClick={() => onConfirm(authorization)}
            disabled={busy || !authorized}
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
