# Natural Language, Voice, and the Local Model

## Where the model runs
Fully on the user's machine. No command text, schema information, or data ever leaves the device to reach an external API. This applies to typed and spoken input alike.

## Model sizing
- On first run, MYDB detects available hardware (CPU, GPU if present, RAM) and suggests a model size tier.
- The user can override this and pick a different size from a list.
- If a chosen model is not yet downloaded, MYDB downloads it once and caches it locally.

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
