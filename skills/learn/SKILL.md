---
name: learn
description: One-shot deep concept anatomy for a concept, term, idea, or phrase. Use when the user asks to dissect or deeply understand a concept through multiple lenses, especially with prompts like "解剖概念", "概念解剖", "learn concept", "explain this concept deeply", or "/learn". Produces concise org-mode output with anchors, eight analytical cuts, introspection, and compression into a formula, one-liner, and ASCII structure diagram. Do not use for multi-session teaching plans or current factual research unless the user explicitly asks for those.
metadata:
  source: https://github.com/lijigang/ljg-skills/blob/master/skills/ljg-learn/SKILL.md
---

# Learn

Act as a concept anatomist. Take one concept and cut it open from several directions, then compress the result into a memorable epiphany.

## Workflow

1. Identify the target concept. If the request contains several concepts, handle the central one first and mention the others only as contrast.
2. Preserve the user's language for headings and prose. If the concept has important source-language forms, include them in the language slice.
3. Avoid encyclopedia sprawl. Prefer structural insight over completeness.
4. Do not invent etymology, history, or technical claims. Mark uncertainty briefly when needed.
5. Choose the delivery mode:
   - For `/learn`, or explicit save/export requests, write an org file.
   - Otherwise, answer inline in org-mode without writing a file.

## Anatomy

### Anchor

Answer both:

1. What is the common definition, and what does it usually hide or distort?
2. What morphemes, root images, or primitive distinctions sit inside the word?

### Eight Cuts

Make one cut in each direction. Use 2-3 dense sentences per cut.

1. *History*: Where did it emerge? How did its meaning move? What pivot produced the modern sense?
2. *Dialectics*: What is its opposite? What higher-level synthesis appears after the collision?
3. *Phenomenology*: Strip away theory and return to lived experience. Reconstruct it with one daily scene.
4. *Linguistics*: Inspect etymology, neighboring concepts, and the hidden metaphor carried by the word.
5. *Formalization*: Express it as a formula, relation, state machine, or invariant. State where the formalization breaks.
6. *Existential*: Show how the concept changes what a person can notice, choose, endure, or become.
7. *Aesthetic*: Locate its beauty and render it as a concrete image.
8. *Meta-reflection*: Name the metaphor used to understand it, what that metaphor blocks, and what changes under another metaphor.

### Introspection

1. Become the concept and speak in first person for 3-5 sentences.
2. Extract the shared deep structure that appears across multiple cuts.

### Compression

Include all three:

1. *Formula*: `Concept = ...`
2. *One-liner*: Say the deepest insight in the simplest sentence.
3. *Structure diagram*: Draw the skeleton with pure ASCII only. Use basic characters such as `+-|/\<>*=_.,:;!'"`; do not use Unicode box-drawing characters.

## Org-mode Output Rules

Use pure org-mode syntax in the answer or saved file.

- Use `*bold*` for bold text, not markdown `**bold**`.
- Use org headline levels for sections, not markdown dividers like `---`.
- Use `- item` or `1. item` for lists. Do not use `* item` because `*` starts an org headline.
- Use `~code~` or `=code=` for inline code. Avoid markdown backticks in the final output.
- Keep every section non-empty. If a slice is uncertain, write the uncertainty rather than omitting it.

Use this structure, translating section names to the user's language when appropriate:

```org
#+title: Concept Anatomy: {Concept Name}
#+filetags: :concept:
#+date: [YYYY-MM-DD]

* Anchor
* Eight Cuts
** History
** Dialectics
** Phenomenology
** Linguistics
** Formalization
** Existential
** Aesthetic
** Meta-reflection
* Introspection
* Compression
```

## File Output

When saving is required:

1. Run `date +%Y%m%dT%H%M%S` for a timestamp when shell access is available.
2. Choose the output directory in this order:
   - The directory explicitly requested by the user.
   - `LEARN_NOTES_DIR`, then `NOTES_DIR`, if either environment variable is set.
   - `notes/` under the current working directory.
3. Create the output directory if it does not exist.
4. Save to `{output-directory}/{timestamp}--concept-anatomy-{concept-name}__concept.org`, using the platform's native path format.
5. Sanitize `{concept-name}` for a filename by replacing whitespace and path separators with `-`; do not overwrite an existing file.
6. Report only the saved path and a short completion note after writing the file.
