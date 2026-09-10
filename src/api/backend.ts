/**
 * The only route from the interface to everything else.
 *
 * There is no database driver in the frontend and no way to reach one
 * (docs/17-coding-standards.md). Every call here goes to a registered Tauri
 * command, which is where the real rules live.
 */
import { invoke } from "@tauri-apps/api/core";

export interface ConnectionSummary {
  id: string;
  name: string;
  engine: string;
  host: string;
  port: number;
  database: string;
  username: string;
  production: boolean;
}

export interface ConnectionInput {
  id?: string;
  name: string;
  host: string;
  port: number;
  database: string;
  username: string;
  password: string;
  production: boolean;
}

export interface TableSummary {
  name: string;
  columns: string[];
}

export interface ActiveConnectionInfo {
  id: string;
  name: string;
  production: boolean;
  tables: TableSummary[];
}

export interface RecordsView {
  columns: string[];
  rows: (string | null)[][];
  totalCount: number;
  truncated: boolean;
  statement: string;
}

export interface ColumnView {
  name: string;
  dataType: string;
  nullable: boolean;
}

/** A table about to be emptied or dropped. */
export interface TableView {
  columns: ColumnView[];
  rowCount: number;
  statement: string;
}

/**
 * What a preview is showing. A schema operation and a record operation are
 * genuinely different questions (docs/04), so they render differently rather
 * than pretending a dropped table is a list of rows.
 */
export type PreviewView =
  | ({ previewKind: "records" } & RecordsView)
  | ({ previewKind: "table" } & TableView);

/**
 * What the user must type before a write on a production-flagged connection
 * can run (docs/11). The interface uses this to disable the confirm action;
 * the backend checks the typed value again, because a gate enforced only in
 * the interface is not a gate.
 */
export type ExtraStep =
  | { kind: "none" }
  | { kind: "tableName"; table: string; prompt: string }
  | { kind: "countOrConfirm"; count: number; prompt: string }
  | { kind: "confirmWord"; prompt: string };

/** Whether what was typed satisfies the step, mirroring the backend rule. */
export function extraStepSatisfied(step: ExtraStep, typed: string): boolean {
  const value = typed.trim();
  switch (step.kind) {
    case "none":
      return true;
    case "tableName":
      return value.length > 0 && value.toLowerCase() === step.table.toLowerCase();
    case "countOrConfirm":
      return value === String(step.count) || value.toLowerCase() === "confirm";
    case "confirmWord":
      return value.toLowerCase() === "confirm";
  }
}

export type CommandOutcome =
  | { kind: "readComplete"; description: string; records: RecordsView }
  | {
      kind: "needsConfirmation";
      description: string;
      /** "delete", "insert", "update", "drop_table", or "truncate". */
      operation: string;
      preview: PreviewView;
      extraStep: ExtraStep;
      destructive: boolean;
      affectsEverything: boolean;
      production: boolean;
    };

export interface ExecutionSummary {
  description: string;
  rowsAffected: number;
}

export const backend = {
  listConnections: () => invoke<ConnectionSummary[]>("list_connections"),
  saveConnection: (input: ConnectionInput) =>
    invoke<ConnectionSummary>("save_connection", { input }),
  deleteConnection: (id: string) => invoke<void>("delete_connection", { id }),
  setProductionFlag: (id: string, production: boolean) =>
    invoke<void>("set_production_flag", { id, production }),
  connect: (id: string) => invoke<ActiveConnectionInfo>("connect", { id }),
  activeConnection: () => invoke<ActiveConnectionInfo | null>("active_connection"),
  submitCommand: (text: string) => invoke<CommandOutcome>("submit_command", { text }),
  editCommand: (text: string) => invoke<CommandOutcome>("edit_command", { text }),
  confirmCommand: (authorization: string) =>
    invoke<ExecutionSummary>("confirm_command", { authorization }),
  cancelCommand: () => invoke<string>("cancel_command"),
};

/** Backend errors arrive as strings, already shaped to be shown to a person. */
export function errorMessage(error: unknown): string {
  return typeof error === "string" ? error : String(error);
}
