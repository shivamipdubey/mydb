# Natural Language, Voice, and the Local Model

## Where the model runs
Fully on the user's machine. No command text, schema information, or data ever leaves the device to reach an external API. This applies to typed and spoken input alike.

## Model sizing
- On first run, MYDB detects available hardware (CPU, GPU if present, RAM) and suggests a model size tier.
- The user can override this and pick a different size from a list.
- If a chosen model is not yet downloaded, MYDB downloads it once and caches it locally.

## Phase 1's parser
Phase 1 uses a rule-based parser, not a model (docs/03-phases-roadmap.md). Accuracy matters more than sophistication at this stage, so it is built around refusing to guess: if it cannot resolve the operation, the table, a column, or a value, it returns an error naming what it could not resolve, along with what does exist. There are no confidence tiers yet; every command either parses into one intent or is rejected.

Two rules the implementation depends on, worth stating because breaking either would be easy and quiet:

- The operation is detected from the command's subject only, never from its filter. Otherwise a value containing a word like "table" changes what operation the command is understood to be.
- Only keyword matching is case-insensitive. Values keep the case the user typed, because an email or a name is data, not syntax.

A parsed filter holds structured, typed values, never a fragment of query text. Adapters bind those values as parameters, so there is nowhere for user input to become syntax (docs/16-security-and-cybersafety-checklist.md item 3).

Operations phase 1 does not parse yet are rejected by name, so the user is told the operation is not supported rather than having it approximated as something else.

## Parsing pipeline
1. Input arrives as text (typed directly, or produced by local speech-to-text from voice).
2. The model produces a structured intent: engine, target, operation, filter or payload.
3. Confidence is scored. High confidence goes straight to the confirmation workflow as a single best-guess query, shown for the user to confirm or edit.
4. Medium confidence shows a short list of alternative interpretations for the user to pick from.
5. Low confidence, or no viable interpretation, triggers a clarifying question back to the user instead of guessing.

## Voice specifics
- Speech-to-text also runs locally.
- Voice input always produces text first, then follows the exact same pipeline as typed input from step 2 onward. Voice is never a shortcut that skips the preview or confirmation steps.

## Testing requirement
docs/18-testing-strategy.md requires test cases at each confidence tier, confirming the correct one of the three fallback behaviors triggers.
