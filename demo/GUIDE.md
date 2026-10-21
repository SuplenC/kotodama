# Kotodama demo guide

Kotodama is a command menu: you type, and it finds and does things. This
desktop is a demo, with a few projects, scripts, clipboard entries, Aliases and
Quicklinks made up for it. Everything here starts from the menu.

Close this window with Ctrl+Shift+Q, or keep it open beside the menu.

## Keys

On the desktop:

- Ctrl+Space: open or close the menu.
- Ctrl+Shift+H: the clipboard history.
- Ctrl+Shift+B: the bookmarks View (the shell example Extension's).
- Ctrl+Return: a terminal (foot). Ctrl+Shift+Q closes a window.

In the menu:

- Up and Down (or Ctrl+P and Ctrl+N) move the selection; PageUp and PageDown
  jump.
- Enter runs the selected Result's primary Action.
- Ctrl+K opens the Action panel: every Action of the Result, then other
  Extensions' Contributed actions.
- Ctrl+Shift+Backspace forgets the selected Result.
- Ctrl+Z, while the Toast shows, puts back what was forgotten.
- Tab, or Enter, on an app's Result enters the app (see below).
- Esc clears a list's filter, pops a View, leaves an app, or closes the menu.

With nothing typed, the menu shows the Results you use most at this time of
day, then a fixed menu: this guide, the clipboard history and "Clear history".

## Finding things

- Matching ignores case and accents, and the words of a query match in any
  order. A single letter matches only at the start of a word.
- Initials of consecutive words match: `fs` finds Foot Server, `fc` Foot Client.
- Typos are forgiven, one per 4 letters, never in the first letter:
  `featerpad` and `fetherpad` find FeatherPad.
- A subtitle is searched once the query has 3 letters.
- A trailing space makes the last word a whole word.

## Every Extension is an app

Each Extension has an app, found by its title and its word: `projects`,
`calc`, `emoji`, `clipboard`, `system`, `web`. Tab or Enter on its Result
enters it: a chip names the app, and before anything is typed a card says
what it does, with a few queries to try, over its commands and the Items
you pick most. Typing then searches that Extension alone. Backspace on the
empty box, or Esc, goes back to the whole search.

Some Items show only in their app: the emoji and the clipboard history stay
out of the search, so nothing there crowds out apps and files.

## Apps

Every desktop app is a Result. Try `fea` (FeatherPad), `foot`, and `htop`,
which runs in a terminal since its desktop file asks for one. On an equal
match an app ranks before other kinds. Emoji stay out of the search, so
`feather` shows FeatherPad and no 🪶.
Ctrl+K on an app shows its desktop actions, if it has any.

## Projects and scripts (the folders Extension)

The demo's `~/Projects` holds a few git repositories:

- `aurora` opens the project in a terminal, in a tmux session named after it.
  Ctrl+K offers "Open Terminal Here", "Open in Lazygit", "Open in File
  Manager", "Copy Path", and "Paste path", which the shell example Extension
  contributes to every project.
- `aurora#aurora-feature-x` is a linked worktree, shown under its repository
  and named after its folder.
- `api` finds `work/api` and `home/api`: two projects with one name, told apart
  by their parent folders.
- `nimbus` and `dotfiles` are plain repositories.

The demo's `~/Scripts` holds executable scripts, each run in a terminal that
stays open:

- `deploy` finds "Deploy staging", named by its header comment
  (`# kotodama: name=Deploy staging`).
- `greet` asks for its arguments in a form first (`args=prompt`).
- `backup` runs in the background as a job (`terminal=false`); the menu lists
  it while it runs.
- `cleanup` shows as "work › cleanup": scripts in subfolders keep their folder.

Add a repository to `~/Projects` or a script to `~/Scripts` from the terminal
and it shows up within a moment.

## Calculator

Its answer takes the top row; Enter copies it, and Ctrl+K offers "Copy
Expression and Answer".

- Math: `2^10`, `(3 + 4) * 2`, `sqrt 2`.
- Units: `12 km in mi`, `100 F to C`, `3 weeks in days`.
- Currency: `100 usd to eur`, at the European Central Bank's rate (it needs
  the network).
- Dates: `today + 3 weeks`, `dec 25 - 1 month`.
- Days between: `days until dec 25`, `days since 2026-01-01`.
- Time zones: `5pm PT in Warsaw`, `now in Tokyo`.
- Anything after `=` that evaluates: `= pi`.

A number alone (`42`) or words get no answer, so they never push other
Results down. In the calculator's app (`calc`, then Tab) everything typed is
evaluated, and its card lists examples such as `15min in sec`.

## Emoji and symbols

Every emoji and the common symbols, in the Emoji app alone: the search
leaves them out and shows the "Emoji" app's Result instead. Type `emoji` and
press Tab, then `fire`, `thumbs`, `like`, `arrow right` or `command` (⌘).
Esc goes back to the search. Enter types the
character into the window the menu was opened from; Ctrl+K offers "Copy" and a
"Type" Action for each skin tone. Open FeatherPad, then the menu, and pick one
to see it typed.

## Clipboard history

Ctrl+Shift+H, or "Clipboard History" in the Clipboard app (`clipboard`, then
Tab), lists what was copied, newest first; a few entries are there from the
start. Typing filters the list. Enter pastes into the window the menu came
from; Ctrl+K offers "Copy to Clipboard", "Show Details" and "Delete from
History". Copy something in foot or FeatherPad and it joins the list. In the
Clipboard app, entries are also found by their text: try `ssh`. The whole
search leaves them out. What reads as a one-time code (six digits alone) or a
secret token (a private key, an API key) is never kept: copy `482913` and it
stays out of the list. `ignore` in `[extensions.clipboard]` sets the regular
expressions that leave text out, and `ignore = []` keeps everything.

## Your keywords: Aliases and Quicklinks

Both live in the demo's config file.

- `ed` is an Alias for FeatherPad: it ranks first, with a badge naming it.
  Words after an Alias are its Result's Argument.
- `aur` is an Alias for one Action of a project, "Open Terminal Here" on
  aurora, which becomes its primary Action.
- `gh` is a Quicklink to GitHub; `github` finds it by its title too.
- `yt cats` reads "Search YouTube for “cats”".
- `say hello` is a Quicklink that runs a command, here a terminal that echoes.
- `guide` is the Quicklink that opened this file.

There is no browser in the demo: whatever would open a web page shows a
terminal saying what it would have opened.

## Windows and power (the system Extension)

Every open window is a Result, found by its title or app id, on any
compositor with wlr-foreign-toplevel-management; this desktop is sway. Open a
terminal or two (Ctrl+Return), then type `foot`: the app comes first, its
windows after it. Enter focuses a window; Ctrl+K offers "Close Window",
"Minimize", "Toggle Maximized" and "Toggle Fullscreen" (sway has no minimizing,
so that one does nothing here). Close a window and its Result goes.

"Power Off", "Reboot", "Suspend", "Lock Screen" and "Log Out" are Results too,
found by `shutdown`, `restart`, `sleep`, `lock` or `logout` as well. Power Off,
Reboot and Log Out ask first in a small form: a second Enter runs them, Esc
leaves. They go through logind, which the demo's container doesn't run, so here
each says why it failed, and Hibernate isn't listed (it is only when logind can
hibernate).

## Views

Extensions push Views on top of the search: lists, forms, details and
progress. Esc pops one at a time, and closing the menu keeps them, so it opens
again where it was.

- Ctrl+Shift+B lists the shell example's bookmarks; typing filters them.
- `timer 5` counts down five seconds in a progress View. Enter on plain
  `timer` asks for the seconds in a form. Its second Action, "Start a labelled
  timer" (Ctrl+L, or Ctrl+K), asks for a label in the Extension's own form:
  Tab moves between fields, Enter submits.
- Ctrl+Shift+H is the clipboard's list View; "Show Details" on an entry is a detail
  View.

## The web

When the real matches leave rows empty, the bottom rows offer "Search
DuckDuckGo for …", Google and Wikipedia: type something nothing matches, such
as `wayland compositors`. A query that looks like an address, such as
`example.com` or `localhost:8080`, opens it from the top row.

## AI

Start the demo with your Claude API key (`ANTHROPIC_API_KEY=sk-ant-... just
demo`), then type `?` and a request in plain words: `? open a terminal`, `?
start a 10 second timer`. The model proposes Actions other Extensions offer;
nothing runs until you press Enter on one. Without a key the AI Extension stays
off.

## Ranking that learns

- Every pick raises its Result (its Frecency), more at the time of day you
  usually pick it.
- Pick the same Result 3 times for one query and it becomes that query's
  Learned choice: it comes first for that query and for the ones that extend
  it. Try picking `foot` from `f` three times, then type `f` again.
- Ctrl+Shift+Backspace (or "Forget" in Ctrl+K) forgets one Result's History.
  A Toast says "Forgot …" for a minute, and Ctrl+Z puts it back. Later, type
  `undo` for the same.
- Ctrl+K also offers "Clear the history of" the Result's Extension.
- "Clear history" (type `clear history`) forgets everything, and can be undone
  the same way.

## What the demo can't show

- Hyprland's keyboard shortcuts, and moving windows between workspaces or
  floating them: those come from the Hyprland Extension, and this desktop is
  sway.
- Powering off, suspending, locking or logging out: the container has no
  logind.
- Web pages: there is no browser.
- Notifications: there is no notification daemon, so a finished job or timer
  says so only in the menu.
- The speed: the menu is drawn in software and sent over VNC.

The full documentation is the README at https://github.com/SuplenC/kotodama.
