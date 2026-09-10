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

export type CommandOutcome =
  | { kind: "readComplete"; description: string; records: RecordsView }
  | {
      kind: "needsConfirmation";
      description: string;
      affected: RecordsView;
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
  confirmCommand: () => invoke<ExecutionSummary>("confirm_command"),
  cancelCommand: () => invoke<string>("cancel_command"),
};

/** Backend errors arrive as strings, already shaped to be shown to a person. */
export function errorMessage(error: unknown): string {
  return typeof error === "string" ? error : String(error);
}
