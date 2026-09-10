import { useState } from "react";

import {
  backend,
  errorMessage,
  type ActiveConnectionInfo,
  type ConnectionSummary,
} from "../api/backend";

const BLANK = {
  name: "",
  host: "localhost",
  port: 5432,
  database: "",
  username: "",
  password: "",
  production: false,
};

/**
 * The connection manager (docs/12-ui-ux-guidelines.md, phase 1 screen).
 *
 * Adds, edits, and removes connections, and sets the production flag.
 * Ungated in phase 1: docs/11 restricts the flag to a connection's admin only
 * once roles exist in phase 4.
 *
 * Phase 1 stores credentials in a plain local file, which docs/06 permits but
 * forbids describing as a vault. The wording below says exactly that, and
 * should not be softened.
 */
export function ConnectionManager({
  connections,
  active,
  onChanged,
  onConnected,
  onError,
}: {
  connections: ConnectionSummary[];
  active: ActiveConnectionInfo | null;
  onChanged: () => void;
  onConnected: (info: ActiveConnectionInfo) => void;
  onError: (message: string) => void;
}) {
  const [form, setForm] = useState({ ...BLANK });
  const [editingId, setEditingId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function save() {
    setBusy(true);
    try {
      await backend.saveConnection({ ...form, id: editingId ?? undefined });
      setForm({ ...BLANK });
      setEditingId(null);
      onChanged();
    } catch (error) {
      onError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function act(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      onError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel" aria-label="Connections">
      <header className="panel-header">
        <h2>Connections</h2>
      </header>

      {connections.length === 0 && (
        <p className="empty-note">No connections saved yet.</p>
      )}

      <ul className="connection-list">
        {connections.map((connection) => (
          <li
            key={connection.id}
            className={active?.id === connection.id ? "connection active" : "connection"}
          >
            <div className="connection-line">
              <strong>{connection.name}</strong>
              {connection.production && (
                <span className="badge badge-production">
                  <span aria-hidden="true">●</span> Production
                </span>
              )}
              {active?.id === connection.id && (
                <span className="badge badge-connected">Connected</span>
              )}
            </div>
            <div className="connection-detail">
              {connection.engine} · {connection.username}@{connection.host}:
              {connection.port}/{connection.database}
            </div>
            <div className="actions">
              <button
                type="button"
                onClick={() =>
                  act(async () => onConnected(await backend.connect(connection.id)))
                }
                disabled={busy}
              >
                Connect
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() =>
                  act(async () => {
                    await backend.setProductionFlag(connection.id, !connection.production);
                    onChanged();
                  })
                }
                disabled={busy}
              >
                {connection.production ? "Unmark production" : "Mark production"}
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() => {
                  setEditingId(connection.id);
                  setForm({
                    name: connection.name,
                    host: connection.host,
                    port: connection.port,
                    database: connection.database,
                    username: connection.username,
                    password: "",
                    production: connection.production,
                  });
                }}
                disabled={busy}
              >
                Edit connection
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() =>
                  act(async () => {
                    await backend.deleteConnection(connection.id);
                    onChanged();
                  })
                }
                disabled={busy}
              >
                Remove
              </button>
            </div>
          </li>
        ))}
      </ul>

      <form
        className="connection-form"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <h3>{editingId ? "Edit connection" : "Add a connection"}</h3>
        <div className="field-grid">
          <label>
            Name
            <input
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              required
            />
          </label>
          <label>
            Host
            <input
              value={form.host}
              onChange={(e) => setForm({ ...form, host: e.target.value })}
              required
            />
          </label>
          <label>
            Port
            <input
              type="number"
              value={form.port}
              onChange={(e) => setForm({ ...form, port: Number(e.target.value) })}
              required
            />
          </label>
          <label>
            Database
            <input
              value={form.database}
              onChange={(e) => setForm({ ...form, database: e.target.value })}
              required
            />
          </label>
          <label>
            Username
            <input
              value={form.username}
              onChange={(e) => setForm({ ...form, username: e.target.value })}
              required
            />
          </label>
          <label>
            Password
            <input
              type="password"
              value={form.password}
              onChange={(e) => setForm({ ...form, password: e.target.value })}
              placeholder={editingId ? "unchanged" : ""}
            />
          </label>
        </div>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={form.production}
            onChange={(e) => setForm({ ...form, production: e.target.checked })}
          />
          Mark as production
        </label>
        <p className="note">
          Passwords are saved to a plain file on this machine, readable only by
          your user account. They are not encrypted. The encrypted vault arrives
          in phase 4.
        </p>
        <div className="actions">
          <button type="submit" disabled={busy}>
            {editingId ? "Save changes" : "Add connection"}
          </button>
          {editingId && (
            <button
              type="button"
              className="secondary"
              onClick={() => {
                setEditingId(null);
                setForm({ ...BLANK });
              }}
            >
              Cancel
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
