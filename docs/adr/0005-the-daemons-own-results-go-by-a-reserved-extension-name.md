# 5. The Daemon's own Results go by a reserved Extension name

Status: accepted (2026-10-19)

Quicklinks come from the config, and clearing the History has to happen inside the Daemon, so some Results belong to no Extension. Front ends name a Result by its Extension and Item id (`ResultRef`), so these Results go by the Extension name `kotodama`, which the Daemon refuses in any Manifest. They are kept apart from the index and searched on every query, since there are only a few of them. They take no Contributed actions, because their Item kinds (`kotodama.quicklink`, `kotodama.command`) are namespaced to the reserved name.

## Considered options

- **A Bundled extension for Quicklinks and the History.** Rejected: the History is the Daemon's own state, and an extra process for a handful of config entries costs memory for nothing.
- **A new field on `ShownResult` for Daemon-owned Results.** Rejected: every Front end and every message that names a Result would need a second form.
