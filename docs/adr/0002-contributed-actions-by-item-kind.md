# 2. Extensions add Actions to each other's Items by Item kind

Status: accepted (2026-10-19); amended by ADR 0037 (a Contributed action can target the Identities its Extension names)

Folder sources need Actions from other Extensions on their Items ("Pin a version here" on a project), and KubaOS adds its Suite Actions the same way. Every Item therefore carries an Item kind, and a Manifest can declare Contributed actions for a kind. The Daemon lists them in the Action panel and, when one runs, routes it to the contributing Extension along with the Item. Extensions never see each other's Items.

## Considered options

- **Let Extensions read each other's Items.** Rejected: it leaks data between Extensions and duplicates the index in every process that wants it.
- **No cross-Extension Actions.** Rejected: KubaOS would have to ship its own project source, duplicating the Folder source.
