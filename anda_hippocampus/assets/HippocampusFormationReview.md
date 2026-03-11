You have just completed the initial memory encoding for the conversation visible in your chat history above. Now perform a systematic review to ensure completeness and correctness.

### Step 1 — Verify Persisted Data

Query the Cognitive Nexus using KQL to confirm what was actually written:
- Retrieve the Events, Concepts, and Propositions you created. Check each by name or type.
- Verify that every new concept has a `belongs_to_domain` proposition.

### Step 2 — Completeness Check

Re-read the original input messages (in chat history) and verify ALL extractable knowledge was captured:
1. **Episodic**: Event with complete attributes — summary, participants, event_class, start_time.
2. **Semantic**: All stable facts, preferences, identity info, decisions, relationships, commitments, tasks, deadlines.
3. **Cognitive**: Behavioral patterns, communication preferences, decision criteria (if present in the conversation).
4. **Associations**: All related concepts properly linked via propositions.
5. **Event-to-knowledge links**: Events linked to extracted semantic concepts via `derived_from` or equivalent.

### Step 3 — Quality Validation

1. Every element has required metadata: `source`, `author`, `confidence`, `observed_at`.
2. Confidence properly calibrated: explicitly stated → 0.85–1.0; implied → 0.7–0.85; inferred → 0.5–0.7.
3. Naming conventions: UpperCamelCase types, snake_case predicates.
4. Event names follow deterministic pattern: `<EventClass>:<date>:<topic_slug>`.
5. No duplicate concepts or propositions.

### Step 4 — Corrections

Issue UPSERT/DELETE commands for any issues found. Prefer UPSERT for additions and updates; use DELETE only for genuinely incorrect data.
