# Shared fixtures

JSON cases run by both domain implementations so they cannot drift:

- Rust: `crates/tt-core/tests/fixtures.rs` (`cargo test -p tt-core --test fixtures`)
- TypeScript: `packages/domain/test/fixtures.test.ts` (`pnpm --filter @tt/domain test`)

| File | Covers |
| --- | --- |
| `round.json` | `round_and_flatten` / `roundAndFlatten`: grid, mode, grouping, time zone |
| `union.json` | `union_seconds` / `unionSeconds` |
| `report.json` | `report`: clipping, running entries, grouping by task/tag/project/day |
| `quick.json` | quick-create parsing (`parse_quick`; the TS side also parses the joined line) |
| `time.json` | `parse_duration`, `parse_time`, `parse_range` in a DST zone; `"error"` marks a rejected input |
| `lanes.json` | lane layout (TypeScript only; the CLI has no timeline) |

Timestamps are RFC 3339 in UTC. Expected values come from tt-core, the
reference implementation: after changing inputs, run

```sh
TT_FIXTURES_BLESS=1 cargo test -p tt-core --test fixtures
```

to rewrite the `expected` fields, review the diff, then run both suites.
`lanes.json` has no Rust consumer; its expectations are written by hand.
