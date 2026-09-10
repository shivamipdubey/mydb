import type { TableView } from "../api/backend";

/**
 * The current shape of a table about to be emptied or dropped.
 *
 * docs/04-database-adapters.md requires a schema operation to preview the
 * table's schema and row count rather than a list of records. For a DROP
 * TABLE this structure is part of what is being lost, so it is shown, not
 * merely counted.
 */
export function TableOutline({ table }: { table: TableView }) {
  return (
    <div className="table-scroll">
      <table className="record-table">
        <thead>
          <tr>
            <th>Column</th>
            <th>Type</th>
            <th>Accepts null</th>
          </tr>
        </thead>
        <tbody>
          {table.columns.map((column) => (
            <tr key={column.name}>
              <td>{column.name}</td>
              <td>{column.dataType}</td>
              <td>{column.nullable ? "yes" : "no"}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
