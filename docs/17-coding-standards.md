# Coding Standards

## Structure
- One adapter per database engine, in its own module, implementing the same interface (connect, describe schema, build preview, execute).
- The confirmation engine, vault, audit and recovery store, and permissions module are each separate modules. No adapter reaches into another adapter's code directly.
- UI code never calls a database driver directly; it only calls through the adapter layer.

## Naming
- Name functions for what they do, not how: buildPreview, not runSelectForPreview, since the preview mechanic varies by engine.
- Keep engine-specific naming inside that engine's adapter file only.

## Error handling
- Every adapter call that can fail must return a typed result (success or failure with a reason), never throw an unhandled exception into the UI layer.
- A failed preview must stop the workflow before reaching confirm; never show a confirm button after a preview error.

## Dependencies
- Prefer well-maintained, widely used libraries for each database driver over writing a custom protocol client.
- Check any new dependency against the security checklist (docs/16-security-and-cybersafety-checklist.md) before adding it.

## Comments
Explain why a safety check exists, not just what the code does, especially around the confirmation workflow and the vault. Future changes should not accidentally remove a safety step because its purpose was not documented in place.
