---
name: modern-web-guidance
description: Look up browser-platform guidance for a frontend implementation or review. Use for HTML semantics, accessibility, forms, CSS layout, browser APIs, performance and compatibility decisions.
license: Apache-2.0 for software and CC BY 4.0 for the guides. See LICENSE.txt and the attribution below.
metadata:
  source: https://github.com/GoogleChrome/modern-web-guidance
---

# Modern web guidance

These guides are bundled with Anda for offline use. Identify the browser behavior in the request and read the relevant guide before choosing an implementation. For example, a modal confirmation maps to [dialog](guides/ui-components/dialog.md), a layout that depends on its container maps to [size-aware styling](guides/css/size-aware-styling.md), and a form error that needs announcing maps to [accessible error announcements](guides/accessibility/accessible-error-announcement.md).

For other topics, look up the path in the [guide index](guide-index.md). Call `skills_read` with `skill` set to `modern-web-guidance` and `resource` set to that package-relative path. Follow `next_cursor` until EOF. When a guide references another guide by its retrieval ID, use the index to open the local file.

Apply the guidance within the project's framework and browser support policy. Check the behavior and any fallback in the target browsers when available. For a decision that depends on today's support, verify against current browser documentation and report what was tested. If the user needs a newer guide, the upstream CLI also supports `modern-web-guidance search` and `retrieve` in an environment where it is installed.

## Attribution

The bundled guides come from [Modern Web Guidance](https://github.com/GoogleChrome/modern-web-guidance) and are distributed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Upstream credits Mozilla contributors and the W3C, WHATWG and IETF specifications. This entry point and the index were adapted for Anda Bot.
