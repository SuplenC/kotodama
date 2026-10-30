# kotodama-calculator code map

A Bundled extension. Maintained by hand: fix stale lines when you find them, and update this map in the PR that adds, moves or renames a module here. Paths are relative to `src/`. `manifest.toml` declares it; `src/main.rs` runs the protocol loop on `kotodama-extension`, reads `Settings`, and fails an Action with `RunError`. See also the [root map](../../CODEMAP.md) and [GLOSSARY.md](../../GLOSSARY.md).

- `lib.rs` (`Calculator::evaluate`, `Answer::live_result`)
- `math.rs` (fend; thousands separators checked before it, `round` trims approximate numbers after it), tests in `math/tests.rs`
- `locale.rs` (`DecimalSeparator`, from the locale or the `decimal_separator` setting), tests in `locale/tests.rs`
- `dates.rs` (the date and time forms: counting days, arithmetic, relative dates, time zones)
- `dates/words.rs` (cleaning a query, filler words, synonyms, one typo in a keyword), tests in `dates/words/tests.rs`
- `dates/calendar.rs` (reading a date: holidays, Easter, weekdays, `NamedDate` from the `dates` setting), tests in `dates/calendar/tests.rs`
- `rates.rs` (ECB rates fetch and cache, in `dirs::cache("calculator")`; `ParseError`, `FetchError`)
- `../tests/answers.rs`: what `Calculator` answers and leaves alone, in process, with the table of ways to ask about dates and times, and that the Manifest's triggers send each.
- `../tests/calculator.rs`: the Extension over its stdio, with a fake rates endpoint on localhost.
- `../benches/answer.rs`: arithmetic answered through the real Daemon over its socket; the p99 must stay under 8 ms.
