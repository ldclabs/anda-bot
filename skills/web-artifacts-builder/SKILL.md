---
name: web-artifacts-builder
description: Build an interactive, multi-component web artifact and deliver a standalone HTML file that opens locally. Use when React state or reusable UI components help with the requested interface.
license: Complete terms in LICENSE.txt
compatibility: The scaffold uses Bash, Node.js, pnpm and tar. Initial setup downloads project-local packages.
---

# Build a standalone HTML artifact

For a small interaction, author a single HTML file in the current Anda workspace. For a multi-component interface, the bundled scaffold supplies React, TypeScript, Tailwind and shadcn/ui components.

1. Use `shell` in the selected workspace. Set `SKILL_DIR` to the actual `base_dir` returned by `skills_manager`. Choose a new project directory. Check the local prerequisites and network access before using the scaffold.
2. Initialize it with `bash "$SKILL_DIR/scripts/init-artifact.sh" my-artifact`. Edit its `src/` files using the workspace tools. Keep images, fonts and other required assets local to the project.
3. From the generated project directory, run `bash "$SKILL_DIR/scripts/bundle-artifact.sh"`. It writes `bundle.html`. Keep the skill directory read-only.
4. Check the resulting HTML for external asset dependencies. When a browser is available, open the built file, exercise the main interaction and check the narrow layout. A Vite development preview alone does not verify the standalone file.
5. Return the absolute path to `bundle.html` and report the checks performed. The file can be opened locally without a hosting service.
