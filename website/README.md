# Anda Bot website

This SvelteKit site serves the Anda Bot landing page, support page, privacy policy and terms at [anda.bot](https://anda.bot). The documentation site lives in [docsite](../docsite/README.md).

Run these commands from the repository root.

```sh
pnpm install --frozen-lockfile
pnpm --dir website dev
```

## Checks and production build

```sh
pnpm --dir website check
pnpm --dir website test
pnpm --dir website lint
pnpm --dir website build
```

The Cloudflare adapter writes the production site to `.svelte-kit/cloudflare` inside `website/`. Use `pnpm --dir website preview` to build and serve it locally with Wrangler.
