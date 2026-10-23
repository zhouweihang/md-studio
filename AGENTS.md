# MD Studio project knowledge

## Commands (run in this directory)

```bash
bun install
bun run tauri dev       # desktop dev; Vite on 1420
bun run test            # Vitest unit tests
bun run typecheck       # tsc --noEmit
bun run build           # tsc + vite build
bun run tauri build     # Windows NSIS installer
bun run check:docs      # docs gate (must PASS)
cd src-tauri && cargo test
```

Toolchain: bun 1.3+, Node 24+, Rust MSVC. `rust-toolchain.toml` in the project root pins MSVC; do not switch to the GNU toolchain for Tauri builds.

## Architecture rules

Dependency direction: `ui → adapters → application → core`. Rust is the filesystem boundary.

- `src/core/` must stay framework-free: no React, DOM, `@tauri-apps/*`, or `invoke`.
- `src/ui/` must not import `@tauri-apps/api/core`; platform calls belong in `src/adapters/tauri/`.
- All note access is `(root, relativePath)`; Rust canonicalizes and rejects `..`, absolute paths, symlink escapes, and non-Markdown extensions. Never bypass `notes.rs` with a new fs command without equivalent checks.
- The filesystem is the source of truth. Do not add a content database/index without a new ADR.
- Markdown preview output must pass through `DOMPurify` before `dangerouslySetInnerHTML`.
- Autosave is 700ms; switching notes, Ctrl+S, and opening a workspace must `flush()` first.

## Documentation workflow (documentation-first)

This project uses the `eng-rfc-adr-c4-specs` + `eng-docs-steward` process:

1. New feature with cross-module impact → RFC in `docs/rfc/RFC-00N-slug.md` with unique `REQ-00N` and executable verification.
2. A decision with alternatives → one ADR in `docs/adr/NNN-slug.md` (filenames are `NNN-` without the `ADR-` prefix; the title is `# ADR-NNN`). Update `docs/adr/README.md`.
3. Structural change → update C1/C2/C3 and append to C3 §6 changelog.
4. Before code → spec snapshot in `docs/specs/YYYY-MM-DD-slug-design.md`; task plan in `docs/plans/YYYY-MM-DD-slug-plan.md`.
5. After implementation → fill RFC §6/§8, close or add backlog items, run `bun run check:docs`.

Gate expectations: ADR filenames `NNN-kebab-slug.md`, RFC filenames `RFC-00N-slug.md`, spec `...-design.md`, plan `...-plan.md`. The script prints the next available ADR/RFC/REQ.

## Verification before claiming done

- `bun run test`, `bun run typecheck`, `bun run build`
- `cd src-tauri && cargo test`
- `bun run check:docs` (ERROR 0)
- For packaging changes: `bun run tauri build` and confirm the NSIS artifact path.
- CI (`.github/workflows/ci.yml`) runs the web tests/build, Rust tests, and the docs gate on push/PR; keep all three green.

Do not weaken path validation or sanitization to make a test pass. Report missing coverage (UI component tests, macOS/Linux) honestly.

## Gotchas

- CRLF: existing files may use CRLF; preserve line endings when editing them.
- CSP in `src-tauri/tauri.conf.json` is restrictive; new external origins need explicit allowance.
- Tauri command arguments are single words today; keep JS/Rust names aligned if adding multi-word args (`camelCase` by default).
- The first Rust release build is slow; debug build is much faster for iteration.
- UI coverage is currently one App flow test (`src/ui/App.test.tsx`); component-level edge cases still need manual or added tests.
