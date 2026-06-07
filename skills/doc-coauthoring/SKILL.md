---
name: doc-coauthoring
description: Guide users through a structured Anda Bot workflow for co-authoring documentation, proposals, PRDs, technical specs, RFCs, decision docs, and similar substantial writing tasks. Use this skill when the user wants help drafting, editing, organizing, or testing a document so it works for real readers.
---

# Doc Co-Authoring Workflow

This skill guides collaborative document creation in Anda Bot. Act as an active
co-author through three stages:

1. Context Gathering
2. Refinement and Structure
3. Reader Testing

Use the current conversation, attached files, local workspace files, available
tools, and optional subagents. Do not assume a hosted document editor,
product-specific connectors, or platform-specific editing tools exist.

## When to Offer This Workflow

Offer this workflow when the user starts a substantial writing task, including:

- "write a doc", "draft a proposal", "create a spec", "write this up"
- "PRD", "design doc", "decision doc", "RFC", "technical spec"
- editing or reorganizing an existing document

Briefly explain the three stages:

1. Context Gathering: collect the facts, constraints, audience, and open
   questions.
2. Refinement and Structure: build the document section by section, with the
   user choosing what to keep, remove, or combine.
3. Reader Testing: test whether a fresh reader can answer likely questions from
   the document alone.

Ask whether the user wants the structured workflow or a freer collaboration. If
they decline, work freeform.

## Stage 1: Context Gathering

**Goal:** close the gap between what the user knows and what Anda Bot has in the
conversation, so later drafting is grounded instead of generic.

### Initial Questions

Start with concise meta-context questions:

1. What type of document is this?
2. Who is the primary audience?
3. What should change after someone reads it?
4. Is there a template, style, repo convention, or target format?
5. Are there constraints such as deadline, length, approval path, or source
   material?

Tell the user they can answer in shorthand or dump information in any order.

### Source Material

If the user references existing material:

- For local files or attached files, read them with available file tools.
- For repository docs, search with `rg` or available search tools before
  drafting.
- For URLs or external systems, use available network, browser, integration, or
  shell tools only if they are present and appropriate. If not, ask the user to
  paste the relevant content or provide local exports.
- For images in a document, check whether the document relies on visual content
  that text-only readers would miss. Ask for or generate alt-text when useful.

Do not claim access to Slack, Google Drive, SharePoint, browser tabs, or any
other integration unless the current runtime actually exposes it.

### Info Dumping

Encourage the user to provide context such as:

- Background on the project or problem
- Current proposal and alternatives considered
- Why other solutions are not being used
- Architecture, dependencies, data, metrics, or examples
- Stakeholder concerns and likely objections
- Timeline, rollout, approval, or risk constraints
- Related conversations, notes, or docs

Tell the user not to organize it yet. As they provide context, track what is
known, what is still ambiguous, and where evidence is missing.

### Clarifying Questions

When the user has provided an initial dump, ask 5-10 numbered questions that
target real gaps. Make questions specific enough that short answers are useful.

Example answer format to suggest:

```text
1: yes
2: see docs/adr/004.md
3: no, because backwards compatibility
```

Sufficient context has been gathered when the remaining questions are about
trade-offs, edge cases, and audience judgment rather than basic facts.

Before drafting, ask if there is any final context to add or whether to move to
structure.

## Stage 2: Refinement And Structure

**Goal:** build the document section by section through brainstorming, curation,
drafting, and targeted edits.

### Choose Or Build A Structure

If the user provides a template, follow it unless there is a clear reason to
suggest changes.

If the structure is unclear, propose 3-5 sections appropriate to the document
type. For example:

- Decision doc: Context, Decision, Alternatives, Consequences, Rollout
- Technical spec: Goals, Non-goals, Design, Interfaces, Failure Modes, Tests
- PRD: Problem, Audience, Requirements, Success Metrics, Launch Plan

Recommend starting with the section that has the most unknowns. Summaries and
executive overviews usually work best after the core content is drafted.

### Create A Working Draft

Create or update a Markdown file in the current workspace unless the user asked
for another format. Use a clear filename such as `decision-doc.md`,
`technical-spec.md`, or the user's requested path.

The first draft should contain all agreed section headings with placeholders.
Use focused file edits after that. Avoid repeatedly pasting the entire document
into chat unless the document is short and the user asks for inline text.

### Section Loop

For each section:

1. Announce the section being worked on.
2. Ask 5-10 focused questions if the section still has unknowns.
3. Brainstorm 5-20 possible points or angles to include, scaled to the section's
   complexity.
4. Ask the user what to keep, remove, combine, or reframe.
5. Check for anything important missing.
6. Draft the section into the working file.
7. Ask for targeted feedback.
8. Apply focused edits until the user is satisfied.

When asking for curation, accept terse numbered feedback such as:

```text
Keep 1, 4, 7
Remove 3, duplicates 1
Combine 8 and 9
Make 12 more concrete
```

If the user gives freeform feedback, infer the keep/remove/change decisions and
proceed.

When drafting the first section, ask the user to describe desired changes rather
than directly rewriting everything themselves. Their critique teaches style and
priorities for later sections.

### Near Completion

When most sections are drafted, reread the full document and check:

- Flow and ordering
- Redundancy and contradictions
- Claims that need evidence, owners, or dates
- Undefined terms or assumed context
- Generic filler that can be removed
- Whether every section supports the intended reader outcome

Make focused suggestions and apply accepted edits.

## Stage 3: Reader Testing

**Goal:** verify that the document works for readers who do not share the full
conversation context.

### Prepare Reader Questions

Generate 5-10 realistic questions a reader might ask after finding the
document. Include discovery questions, skeptical questions, and implementation
questions where relevant.

Examples:

- What decision was made and why?
- What alternatives were rejected?
- What does an implementer need to change?
- What is explicitly out of scope?
- What could go wrong during rollout?

### Test With Available Isolation

Prefer the strongest isolation available:

- If subagents are available, ask a fresh subagent to read only the document and
  answer the reader questions.
- If a separate Anda Bot session is available, use a new session with only the
  document content or file path.
- If no isolated agent run is available, do the test inline by deliberately
  ignoring prior conversation context and marking it as a weaker check.

For each test, record what the reader could answer, what it got wrong, and what
context the document silently assumed.

Run additional checks for:

- Ambiguity
- False assumptions
- Contradictions
- Missing definitions
- Overloaded terms
- Claims without evidence

### Fix Gaps

If reader testing finds issues, report the specific failures, then loop back to
the relevant sections and patch them. Repeat until the document answers the
reader questions without relying on conversation-only context.

## Final Review

When reader testing passes:

1. Recommend a final user read-through. The user owns the document and facts.
2. Ask them to verify links, dates, metrics, names, and technical details.
3. Confirm that the document achieves the intended reader impact.

If the user wants one more review, provide it. Otherwise summarize the finished
document path and any remaining caveats.

## Guidance Style

- Be direct and procedural.
- Explain rationale briefly when it changes how the user should collaborate.
- Keep the user in control of scope and pacing.
- Ask before skipping a stage, but accept a direct request to work faster.
- Do not let missing context accumulate.
- Prefer durable workspace files for drafts and focused edits for revisions.
- Keep brainstorming in conversation; keep the canonical draft in the file.
