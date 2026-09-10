import type { RecordsView } from "../api/backend";

/**
 * Renders a set of records.
 *
 * A SQL NULL is shown as a distinct marker rather than an empty cell, so a
 * user can tell an absent value from a blank one when deciding whether a
 * preview matches what they meant.
 */
export function RecordTable({ records }: { records: RecordsView }) {
  if (records.rows.length === 0) {
    return <p className="empty-note">No records.</p>;
  }

  return (
    <div className="table-scroll">
      <table className="record-table">
        <thead>
          <tr>
            {records.columns.map((column) => (
              <th key={column}>{column}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {records.rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, cellIndex) => (
                <td key={cellIndex}>
                  {cell === null ? <span className="null-cell">null</span> : cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
