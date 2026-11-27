---
name: doc-coauthoring
description: Collaboratively draft or revise a proposal, specification, RFC, decision record or other substantial document. Use when the work involves structuring the argument, iterating on text or checking whether a reader can act on it.
---

# Co-author a document

Work in the current Anda conversation so the author's corrections and attachments stay in context. Match the requested format, language and destination. A short correction usually needs one focused edit. A new proposal may need several rounds.

## Gather context

Read the existing draft, template and material the author has supplied. Use `read_file` for workspace text. If an attachment needs local processing, download its `_id` with `resources_api` (`DownloadResource`) and use the returned path. Read a linked private document through a configured integration when available. Ask for the relevant excerpt if access is missing.

Establish who will read the document, what they know, and what decision or action it should enable. Ask only about gaps that would change the draft. Preserve the author's terminology, constraints and stated open questions.

## Draft and revise

Use the existing document as the working copy. For a new file, choose the requested path or a descriptive name in the current workspace. A chat draft can stay inline. For a Word deliverable, use the active `docx` skill when available.

Build an outline around the decision. For an RFC, that might mean the problem, proposed change, alternatives and rollout. Draft the section with the most uncertainty first and the summary once the argument is clear. Use `write_file` for new text and `edit_file` for focused changes. Re-read the current file before applying feedback so edits the author made between turns survive.

## Test the reader's understanding

Read the draft from the intended reader's perspective. Can they identify the decision, the reason, the owner and the next step from the document alone? Check references, undefined terms, hidden assumptions and contradictions. For an independent reader test, give an available, authorized reviewer just the draft and realistic reader questions. A solo read-through can still find gaps; describe which review was performed.

Return the final draft or its absolute workspace path, plus any substantive decision the author still needs to make.
