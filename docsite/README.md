# Anda Bot Docs

This is the Docusaurus documentation site for Anda Bot. Production URL: `https://docs.anda.bot`.

## Installation

```bash
pnpm install
```

## Local Development

```bash
pnpm start
```

This starts a local Docusaurus development server. Most changes are reflected live without restarting the server.

## Build

```bash
pnpm build
```

This generates static content into `build/`.

## Internationalization

The docs use the same locale set as the main `website/`: English, Chinese, Spanish, French, Russian, and Arabic. English is the fallback locale. All non-English locales have localized docs under `i18n/<locale>/docusaurus-plugin-content-docs/current/`.

```bash
pnpm start -- --locale zh-Hans
pnpm write-translations -- --locale zh-Hans
```

When adding a new locale, keep `docusaurus.config.ts`, homepage copy, and `i18n/<locale>/` translation files aligned.

## Deployment

The site is configured for the custom domain `docs.anda.bot` in `docusaurus.config.ts`. Deploy `build/` to the hosting provider behind that domain.

## Content Direction

- Keep install commands aligned with the repository README and release scripts.
- Keep the visual language aligned with the main `website/`: memory observatory, dark forest ink, amber and teal signal colors.
- Explain Hippocampus through formation, recall, and maintenance.
- Avoid reintroducing generic Docusaurus tutorial or blog template content.
