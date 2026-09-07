---
title: Translating FluxDown
description: Help translate the app, the web UI, and this website by contributing on GitHub.
section: contributing
order: 2
---

FluxDown translations are maintained in the public [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown) repository. Community members contribute language updates through GitHub pull requests.

## What can be translated


| Component | What it covers |
| --- | --- |
| **Desktop & Mobile App** | Every string in the Windows/macOS/Linux app and the mobile app |
| **Web App** | The web UI served by the headless server |
| **Website** | fluxdown.zerx.dev — landing page, FAQ, changelog |

English is the source language; Simplified Chinese is maintained by the core team. Everything else is yours to build.

## Quick start

1. Fork [zerx-lab/FluxDown](https://github.com/zerx-lab/FluxDown) and create a branch from `main`.
2. Add or update the relevant translation file:
   - **Desktop & Mobile App**: `assets/i18n/`
   - **Web App**: `web/src/lib/locales/`
   - **Website**: `website/src/lib/locales/`
3. Open a pull request against `main`. A maintainer will review and merge it.

## Placeholders

Text in curly braces like `{name}`, `{count}`, or `{speed}` is replaced with live values at runtime. **Keep placeholders exactly as-is** — reposition them freely to fit your language's grammar, but never translate or delete what's inside the braces.

## Starting a new language

Your language isn't listed yet? Add a translation file for it in the relevant component directory and include `languageNativeName` first so the language selector can label it correctly. Then open a pull request against `main`.

After it merges:

- **App**: your language appears automatically in *Settings → Language* in the next release.
- **Web UI & website**: the language shows up in the language switcher with the next deploy.

## Tips

- **Partial translations are fine.** Untranslated strings fall back to English key by key — a 30% translated language is already useful.
- **Consistency beats literalness.** Check nearby strings and reuse the same term for the same concept.
- Found a typo in the English source? Include the correction in your pull request, or open an issue.
