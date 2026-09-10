/**
 * Screen-level tests for the safety loop.
 *
 * These mock the Tauri command bridge, so they run everywhere including
 * macOS, where `tauri-driver` cannot run at all (docs/18-testing-strategy.md).
 * They cover what the user sees and which commands the interface sends; the
 * real backend behaviour is covered by the Rust integration tests.
 */
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";

const CONNECTION = {
  id: "c1",
  name: "Local test",
  engine: "PostgreSQL",
  host: "localhost",
  port: 55432,
  database: "mydb_test",
  username: "mydb_test",
  production: false,
};

const AFFECTED = {
  previewKind: "records" as const,
  columns: ["id", "email"],
  rows: [
    ["3", "alan@example.com"],
    ["6", "barbara@example.com"],
  ],
  totalCount: 2,
  truncated: false,
  statement: 'SELECT "id"::text, "email"::text FROM "public"."users" WHERE "active" = $1',
};

const NEEDS_CONFIRMATION = {
  kind: "needsConfirmation",
  description: "Delete records in users where active is false",
  operation: "delete",
  preview: AFFECTED,
  extraStep: { kind: "none" },
  destructive: true,
  affectsEverything: false,
  production: false,
};

/** Records every command the interface sends, so order can be asserted. */
let sent: string[] = [];

function mockBackend(overrides: Record<string, unknown> = {}) {
  sent = [];
  mockIPC((cmd, args) => {
    sent.push(cmd);
    if (cmd in overrides) {
      const value = overrides[cmd];
      return typeof value === "function"
        ? (value as (a: unknown) => unknown)(args)
        : value;
    }
    switch (cmd) {
      case "list_connections":
        return [CONNECTION];
      case "active_connection":
        return { id: "c1", name: "Local test", production: false, tables: [] };
      case "submit_command":
      case "edit_command":
        return NEEDS_CONFIRMATION;
      case "confirm_command":
        return {
          description: "Delete records in users where active is false",
          rowsAffected: 2,
          historyWarning: null,
        };
      case "cancel_command":
        return "Delete records in users where active is false";
      default:
        return null;
    }
  });
}

beforeEach(() => {
  vi.stubGlobal("crypto", { ...globalThis.crypto });
});

afterEach(() => {
  clearMocks();
});

async function submit(text: string) {
  const user = userEvent.setup();
  render(<App />);
  await screen.findByText(/Connected to/);
  await user.type(screen.getByLabelText("Command"), text);
  await user.click(screen.getByRole("button", { name: "Run" }));
  return user;
}

describe("the confirmation loop", () => {
  it("shows what a delete would affect, in plain language, before anything runs", async () => {
    mockBackend();
    await submit("delete users where active is false");

    expect(
      await screen.findByText("Delete records in users where active is false"),
    ).toBeInTheDocument();
    expect(screen.getByText("2 records will be deleted.")).toBeInTheDocument();

    // The actual records, not just a count.
    expect(screen.getByText("alan@example.com")).toBeInTheDocument();
    expect(screen.getByText("barbara@example.com")).toBeInTheDocument();

    // And nothing has run.
    expect(sent).not.toContain("confirm_command");
  });

  it("offers confirm, edit, and cancel", async () => {
    mockBackend();
    await submit("delete users where active is false");

    await screen.findByRole("button", { name: "Confirm" });
    expect(screen.getByRole("button", { name: "Edit command" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("labels a destructive change with more than colour", async () => {
    mockBackend();
    await submit("delete users where active is false");
    // docs/12: colour alone is never the only signal.
    expect(await screen.findByText(/Destructive/)).toBeInTheDocument();
  });

  it("runs the write only after confirm is pressed", async () => {
    mockBackend();
    const user = await submit("delete users where active is false");

    await user.click(await screen.findByRole("button", { name: "Confirm" }));

    await waitFor(() => expect(sent).toContain("confirm_command"));
    expect(sent.indexOf("submit_command")).toBeLessThan(sent.indexOf("confirm_command"));
    expect(await screen.findByText(/2 records affected/)).toBeInTheDocument();
  });

  it("cancel discards the change and says nothing was altered", async () => {
    mockBackend();
    const user = await submit("delete users where active is false");

    await user.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(await screen.findByText(/Nothing was changed/)).toBeInTheDocument();
    expect(sent).toContain("cancel_command");
    expect(sent).not.toContain("confirm_command");
  });

  it("edit returns to a fresh preview rather than executing", async () => {
    mockBackend();
    const user = await submit("delete users where active is false");

    await user.click(await screen.findByRole("button", { name: "Edit command" }));
    const field = await screen.findByLabelText("Revise the command");
    await user.clear(field);
    await user.type(field, "delete users where id is 3");
    await user.click(screen.getByRole("button", { name: "Preview again" }));

    await waitFor(() => expect(sent).toContain("edit_command"));
    expect(sent).not.toContain("confirm_command");
    expect(await screen.findByRole("button", { name: "Confirm" })).toBeInTheDocument();
  });

  it("calls out a filter that matches the whole table", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        description: "Delete every record in users",
        affectsEverything: true,
      },
    });
    await submit("delete all users");

    expect(await screen.findByText(/affects every\s+record in the table/)).toBeInTheDocument();
  });

  it("shows a parse error instead of guessing", async () => {
    mockBackend({
      submit_command: () => {
        throw "no table called \"invoices\". This connection has: users, orders";
      },
    });
    await submit("delete every invoice");

    expect(await screen.findByRole("alert")).toHaveTextContent(/no table called/);
    expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
  });

  it("an insert says the record will be created, not affected", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        kind: "needsConfirmation",
        operation: "insert",
        description: "Add one record to users, with email = \"z@example.com\"",
        destructive: false,
        preview: { ...AFFECTED, rows: [["8", "z@example.com"]], totalCount: 1 },
      },
    });
    await submit("add a user with email is z@example.com");

    expect(await screen.findByText("1 record will be created.")).toBeInTheDocument();
    // An insert destroys nothing, so it must not be labelled destructive.
    expect(screen.queryByText(/Destructive/)).not.toBeInTheDocument();
  });

  it("an update says the records will be updated", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        operation: "update",
        description: "Update records in users where id is 3, setting active = false",
      },
    });
    await submit("update users set active to false where id is 3");

    expect(await screen.findByText("2 records will be updated.")).toBeInTheDocument();
    // An update overwrites what was there, so it is destructive.
    expect(screen.getByText(/Destructive/)).toBeInTheDocument();
  });

  it("a drop shows the table's structure and says the table itself goes", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        operation: "drop_table",
        description: "Drop the table users, removing its records and its structure",
        preview: {
          previewKind: "table",
          columns: [
            { name: "id", dataType: "integer", nullable: false },
            { name: "email", dataType: "text", nullable: false },
          ],
          rowCount: 7,
          statement: 'SELECT count(*) AS total FROM "public"."users"',
        },
      },
    });
    await submit("drop table users");

    expect(await screen.findByText("7 records will be deleted.")).toBeInTheDocument();
    // The structure is part of what is lost, and a record count alone
    // would not tell the user that.
    expect(
      screen.getByText(/removes the table itself, not\s+just its records/),
    ).toBeInTheDocument();
    expect(screen.getByText("integer")).toBeInTheDocument();
  });

  it("a truncate keeps the table and says so", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        operation: "truncate",
        description: "Remove every record from users, keeping the table itself",
        preview: {
          previewKind: "table",
          columns: [{ name: "id", dataType: "integer", nullable: false }],
          rowCount: 7,
          statement: 'SELECT count(*) AS total FROM "public"."users"',
        },
      },
    });
    await submit("truncate users");

    expect(await screen.findByText("7 records will be deleted.")).toBeInTheDocument();
    expect(
      screen.getByText(/Remove every record from users, keeping the table itself/),
    ).toBeInTheDocument();
    // A truncate leaves the table, so it must not carry the drop warning.
    expect(screen.queryByText(/removes the table itself/)).not.toBeInTheDocument();
  });

  // --- docs/11: the production flag's extra step ---

  it("a production delete keeps confirm disabled until the count or CONFIRM is typed", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        production: true,
        extraStep: {
          kind: "countOrConfirm",
          count: 2,
          prompt: "This connection is flagged production. Type 2 or the word CONFIRM to continue.",
        },
      },
    });
    const user = await submit("delete users where active is false");

    const confirm = await screen.findByRole("button", { name: "Confirm" });
    expect(confirm).toBeDisabled();

    // A wrong entry does not unlock it.
    const field = screen.getByLabelText("Record count, or CONFIRM");
    await user.type(field, "1");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();

    await user.clear(field);
    await user.type(field, "2");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Confirm" }));
    await waitFor(() => expect(sent).toContain("confirm_command"));
  });

  it("a production delete of one record will not accept the count", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        production: true,
        extraStep: {
          kind: "confirmWord",
          prompt: "This connection is flagged production. Type the word CONFIRM to continue.",
        },
      },
    });
    const user = await submit("delete users where id is 3");

    const field = await screen.findByLabelText("Type CONFIRM");
    await user.type(field, "1");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();

    await user.clear(field);
    await user.type(field, "CONFIRM");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeEnabled();
  });

  it("a production drop is gated on the table name, not the record count", async () => {
    mockBackend({
      submit_command: {
        ...NEEDS_CONFIRMATION,
        operation: "drop_table",
        production: true,
        description: "Drop the table users, removing its records and its structure",
        preview: {
          previewKind: "table",
          columns: [{ name: "id", dataType: "integer", nullable: false }],
          rowCount: 7,
          statement: "SELECT count(*)",
        },
        extraStep: {
          kind: "tableName",
          table: "users",
          prompt:
            "This connection is flagged production. Type the table's name, users, to continue.",
        },
      },
    });
    const user = await submit("drop table users");

    const field = await screen.findByLabelText("Table name");
    // The row count is not a way past this gate.
    await user.type(field, "7");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeDisabled();

    await user.clear(field);
    await user.type(field, "users");
    expect(screen.getByRole("button", { name: "Confirm" })).toBeEnabled();
  });

  it("an unflagged connection asks for nothing extra", async () => {
    mockBackend();
    await submit("delete users where active is false");

    expect(await screen.findByRole("button", { name: "Confirm" })).toBeEnabled();
    expect(screen.queryByLabelText(/CONFIRM|Table name/)).not.toBeInTheDocument();
  });

  it("a read shows its result with no confirmation step", async () => {
    mockBackend({
      submit_command: {
        kind: "readComplete",
        description: "Show every record in users",
        records: AFFECTED,
      },
    });
    await submit("show me all users");

    expect(await screen.findByText("2 records found.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
  });
});
