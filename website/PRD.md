# Anda Bot Website Design And Requirements

Status: living document
Owner: Anda Bot website maintainers
Last updated: 2026-04-30
Primary route: `/`
Current implementation: SvelteKit, Svelte 5, Tailwind CSS v4, shadcn-svelte-style local primitives

This document is the product, design, and implementation reference for the Anda Bot website. Keep it current as the site evolves. The goal is to make future edits intentional instead of drifting into a generic AI landing page.

## 1. Product Context

Anda Bot is an open-source agent with a long-term memory brain powered by Anda Hippocampus. The website should introduce Anda Bot from the bot's own point of view and help visitors understand why persistent memory matters.

The site is not a developer API reference. It is a product-facing first impression for people asking:

- What is Anda Bot?
- Why is it different from other agents?
- What does long-term memory actually do for me?
- Can I run it locally and trust what it is doing?
- Where do I go next?

## 2. Positioning

Core promise:

> Anda Bot is the agent that remembers. It keeps the thread of your work alive across conversations, tools, files, and channels.

Primary differentiator:

- Anda Bot is centered on Anda Hippocampus, a living long-term memory system with formation, recall, and maintenance, rather than a thin chat wrapper with temporary context.

Supporting differentiators:

- Local-first runtime and workspace.
- Open-source components that can be inspected.
- Works through terminal, chat channels, voice, files, tools, and scheduled workflows.
- Designed for continuity across real work, not one-off prompts.

## 3. Audience

Primary audience:

- Builders, researchers, and power users who want an AI agent with durable context.
- Developers and technically capable users evaluating whether to install or contribute.
- Early adopters comparing Anda Bot with other agent frameworks.

Secondary audience:

- Community members who arrive from GitHub, social posts, docs, or search.
- Future users who are not yet ready to install but need a memorable product story.

Reader assumptions:

- They have seen many AI agents.
- They are skeptical of vague claims.
- They respond well to concrete primitives: memory, local workspace, channels, commands, open source.

## 4. Website Goals

The current site should accomplish these outcomes:

- Make "Anda Bot" and "long-term memory" obvious in the first viewport.
- Explain Anda Hippocampus as the heart of the product without making visitors read a technical paper.
- Create a distinctive visual impression that feels cognitive, alive, and practical.
- Convert interested users toward GitHub, quick start, or the Anda Hippocampus repository.
- Establish a design language that can support future pages, docs, examples, and product surfaces.

Non-goals for the current version:

- Do not become a marketing-only hero page with no product substance.
- Do not duplicate the full developer documentation.
- Do not overclaim production readiness or enterprise features that are not implemented.
- Do not use a generic purple AI gradient, abstract stock imagery, or empty futuristic copy.

## 5. Current Page Map

The current implementation is a single landing page with four major bands.

### 5.1 Hero

Purpose:

- Establish product identity and emotional hook immediately.
- Present the site in Anda Bot's first-person voice.
- Show the dynamic memory graph as the first visual asset.

Current content:

- Badge: `Long-term memory agent`
- H1: `Anda Bot`
- Core line: `I am the agent that remembers...`
- CTAs: `Run me locally`, `Meet my memory`, `GitHub`
- Proof points: `3 memory phases`, `local workspace`, `open source`
- Desktop-only memory console: `hippocampus.stream`, recall pulse, memory signal tags, example `anda chat` output

Requirements:

- The product name must be visible in the first viewport.
- The hero must include a visual memory system, currently `NexusCanvas`.
- Mobile must show a hint of the next section in the first viewport.
- CTA labels must remain action-oriented and concise.

### 5.2 Memory Section

Purpose:

- Explain the Anda Hippocampus mental model.
- Turn "memory" from a vague claim into a product mechanism.

Current content:

- Section badge: `Anda Hippocampus`
- Headline: `My memory is not a note pile. It is a living loop.`
- Three cards: `Formation`, `Recall`, `Maintenance`

Requirements:

- Keep the three-phase model visible and easy to scan.
- Avoid overloading this section with implementation details.
- Link or route deeper later when dedicated Hippocampus content exists.

### 5.3 Workflows Section

Purpose:

- Show where Anda Bot can be useful in daily work.
- Bridge the conceptual memory story with practical channels and tools.

Current content:

- Badge: `Where I work`
- Headline: `Bring me to the places your context is born.`
- Tags: chat, voice, shell, channels
- Cards: local-first shell, multi-channel chat, your keys/models, inspectable memory

Requirements:

- Keep examples concrete.
- Prefer capability categories over long integration lists.
- If adding new integrations, include them only when they are implemented or clearly documented.

### 5.4 Start Section

Purpose:

- Convert interest into installation or repository exploration.
- Show that the product is real and runnable.

Current content:

- Badge: `Start a persistent thread`
- Headline: `Install me, give me a model, then talk to me like a teammate.`
- CTAs: `Open repository`, `Quick start`
- Terminal snippet:

```sh
git clone https://github.com/ldclabs/anda-bot
cd anda-bot
cargo install --path anda_bot

anda chat
```

Requirements:

- Install instructions must match the current repository docs.
- The command block must wrap on mobile and avoid horizontal page overflow.
- If installation changes, this section should be updated in the same pull request.

## 6. Voice And Messaging

The site speaks as Anda Bot. This is intentional.

Voice qualities:

- First-person, calm, capable, slightly intimate.
- Specific about memory and work.
- Confident without sounding omniscient.
- Practical enough for technical readers.

Preferred phrasing:

- `I remember...`
- `My Hippocampus brain...`
- `I keep the thread of your work alive...`
- `Bring me to the places your context is born...`

Avoid:

- Generic claims like `supercharge productivity` or `10x your workflow`.
- Dense technical jargon in the hero.
- Treating Anda Bot as only a chatbot.
- Explaining every UI affordance in visible page copy.

## 7. Visual Design Direction

Design concept:

- A cognitive instrument panel for an agent with a living memory brain.
- Organic, dark, warm, and technical.
- More like a memory observatory than a SaaS dashboard.

Current palette:

- Ink: `#07110f`
- Forest: `#10201c`
- Amber: `#f1a64e`
- Amber soft: `#ffd08a`
- Teal: `#35b2ab`
- Clay: `#d84b42`
- Parchment: `#f4eee3`
- Lichen: `#a9c9a3`
- Muted: `#b9c5b6`

Typography:

- Body: `Inter Variable`, provided by `@fontsource-variable/inter`.
- Display: `.anda-display`, currently a serif stack using `Iowan Old Style`, Palatino, and Georgia.

Shape language:

- Radius should stay at or below the shadcn default `lg` scale for cards and panels.
- Use restrained glass panels, not nested decorative card stacks.
- Icons should come from `@lucide/svelte` when possible.

Texture and motion:

- `NexusCanvas` supplies the hero visual asset and interactive memory field.
- `.anda-grain` adds a subtle grid texture.
- Motion must respect `prefers-reduced-motion`.

Avoid:

- Purple-blue gradients as the dominant look.
- One-note dark blue/slate palettes.
- Decorative blobs or generic abstract AI orbs.
- Stock imagery that does not reveal the product or memory idea.

## 8. Component Requirements

Current local primitives:

- `src/lib/components/ui/button/button.svelte`
- `src/lib/components/ui/badge/badge.svelte`
- `src/lib/components/ui/card/card.svelte`
- `src/lib/components/landing/NexusCanvas.svelte`

Button requirements:

- Variants: `primary`, `secondary`, `ghost`.
- Sizes: `default`, `sm`.
- Must support internal hash links and external links.
- External links should use `rel="noreferrer"` when `target="_blank"` is set.

Badge requirements:

- Tones: `warm`, `cool`, `ink`.
- Must support inline icon + text layouts.

Card requirements:

- Use for repeated item cards or framed tool surfaces.
- Do not place cards inside other cards.

NexusCanvas requirements:

- Canvas must render a nonblank animated memory field.
- Must resize with its container.
- Must stay decorative to screen readers via `aria-hidden="true"`.
- Must not block content or controls.
- Must preserve acceptable performance on mobile and desktop.

## 9. Functional Requirements

Navigation:

- Header brand links to `/`.
- Desktop nav links to `#memory`, `#work`, and `#start`.
- GitHub CTA links to `https://github.com/ldclabs/anda-bot`.
- Hippocampus CTA links to `https://github.com/ldclabs/anda-hippocampus`.

SEO and metadata:

- Page title must include `Anda Bot`.
- Meta description must mention long-term memory and Anda Hippocampus.
- Open Graph title and description must be present.
- Future iteration should add a designed OG image.

Responsive behavior:

- Minimum supported width: 320px.
- Mobile hero should not exceed the first viewport so much that the next section disappears entirely.
- Desktop hero may show the memory console; mobile currently hides it to preserve pacing.
- No horizontal page overflow at 390px width.

Accessibility:

- Text contrast should remain readable over animated backgrounds.
- Interactive elements need visible focus states.
- Decorative canvas is hidden from assistive tech.
- Copy should not depend on color alone to convey meaning.

Performance:

- The page should build without Svelte diagnostics.
- The canvas should cap device pixel ratio to avoid excessive memory use.
- Animation should stop or reduce appropriately for reduced motion users.
- Avoid adding heavyweight animation libraries unless they unlock a clear product effect.

## 10. Technical Requirements

Stack:

- SvelteKit
- Svelte 5 runes mode
- TypeScript
- Tailwind CSS v4
- shadcn-svelte styling conventions
- `@lucide/svelte` for icons

Important files:

- `src/routes/+page.svelte`: landing page structure and content
- `src/routes/layout.css`: global CSS variables, shadcn theme, Anda brand tokens
- `src/lib/components/landing/NexusCanvas.svelte`: hero canvas asset
- `src/lib/components/ui/*`: local UI primitives
- `package.json`: scripts and dependencies

Add components from shadcn-svelte:

```sh
pnpm dlx shadcn-svelte@latest add [component]
```

All components: https://www.shadcn-svelte.com/docs/components.md

Validation commands:

```sh
pnpm check
pnpm build
```

Local development:

```sh
pnpm dev
```

Browser validation checklist:

- Desktop page loads without console errors.
- Mobile page has no horizontal overflow.
- Canvas is visibly nonblank.
- Hero CTA links work.
- Section hash links scroll to the expected content.
- Text does not overlap or clip at common widths: 320, 390, 768, 1024, 1440.

## 11. Iteration Backlog

High priority:

- Add a real OG image for social sharing.
- Add a dedicated Hippocampus explainer route or section with diagrams.
- Add installation variants once release packaging is finalized.
- Add concise Chinese copy or a localized route if the site needs bilingual entry points.

Medium priority:

- Add use-case modules for personal memory, project memory, team channels, and autonomous routines.
- Add a comparison section that explains why long-term memory is different from context windows or simple vector search.
- Add screenshots or short motion captures of the TUI, chat channels, and voice flow.
- Add footer links for README, GitHub, Hippocampus, issues, license, and community.

Low priority:

- Add analytics with privacy-aware defaults.
- Add theme tokens for light-mode documentation pages.
- Add automated visual regression tests for the landing page.

## 12. Open Questions

- Should `anda.bot` be English-only at launch, bilingual, or language-detected?
- What is the preferred primary conversion: install command, GitHub star, docs, or hosted demo?
- Will there be a hosted Anda Bot experience, or should all copy emphasize local-first usage?
- Which model providers should be named directly on the website?
- Should the site include a public roadmap or stay focused on the current product surface?
- What is the canonical logo or wordmark direction beyond the current icon treatment?

## 13. Definition Of Done For Future Website Changes

A website change is ready when:

- It strengthens the memory-first positioning.
- It does not make unsupported claims.
- It passes `pnpm check`.
- It passes `pnpm build`.
- It has been checked on mobile and desktop viewports.
- It preserves or improves accessibility.
- It updates this document when product story, information architecture, or design rules change.

## 14. Changelog

### 2026-04-30

- Created the first living design and requirements document for the Anda Bot website.
- Documented the current single-page landing experience, design direction, component requirements, validation commands, and iteration backlog.
