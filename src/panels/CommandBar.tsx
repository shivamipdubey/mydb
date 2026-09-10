import { useState, type FormEvent } from "react";

/**
 * The command bar (docs/12-ui-ux-guidelines.md): always visible, where typed
 * input is entered. Voice input joins this same bar in phase 5.
 */
export function CommandBar({
  onSubmit,
  disabled,
  busy,
  placeholder,
}: {
  onSubmit: (text: string) => void;
  disabled: boolean;
  busy: boolean;
  placeholder: string;
}) {
  const [text, setText] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    const trimmed = text.trim();
    if (trimmed.length > 0 && !disabled && !busy) {
      onSubmit(trimmed);
    }
  }

  return (
    <form className="command-bar" onSubmit={submit}>
      <input
        type="text"
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder={placeholder}
        disabled={disabled}
        aria-label="Command"
        autoFocus
      />
      <button type="submit" disabled={disabled || busy || text.trim().length === 0}>
        {busy ? "Working…" : "Run"}
      </button>
    </form>
  );
}
