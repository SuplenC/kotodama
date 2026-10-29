# 40. The Daemon checks a submitted form and names each wrong field

Status: accepted (2026-10-29)

A form asked for text, a checkbox, a select or a file, and every Extension checked the answers itself after submit: whether a required field was filled in, whether text that should be a number was one. Each did it its own way, and none could keep the form open and point at the field, since the form was the Front end's and the answer came back as a failed Action. Forms gain a text area, a number, a date, a multi-select and a description, and every field gains `optional` and `pattern`, so the checks that every Extension wrote can be made once, in the Daemon, before the Extension sees the form.

## The kinds

All are in protocol 1.0, added in place, as the file input was, since 1.0 isn't released.

- **`TextArea { placeholder, default }`**, submitted as a string.
- **`Number { min, max, step, default }`**, each an `f64` or none. `Value` already has `Integer` and `Float`, and timers and brightness are whole while zoom and money aren't, so the bounds are floats and the field takes either. The value reaches the Extension as `Integer` when it is whole (below 2^53, where every whole float is an `i64` exactly) and `Float` otherwise: a shell script's `jq -r` prints `3`, not `3.0`, and a Rust Extension reads a whole number without a cast. `step` counts from `min`, or from 0 without one, as HTML's number input does. Without `step` any number goes, and a Front end's arrows step by 1. A step is checked with a slack of 1e-9 of the number of steps, since `0.1 * 3` isn't `0.3` in floats and the error grows with the value.
- **`Date { time, default }`**, submitted as ISO 8601 in local time with no offset: `2026-10-30`, or with `time` `2026-10-30T09:30`, seconds allowed. Local and offset-free because a person picks a wall-clock moment ("remind me at 9:30"), and the Extension knows better than the Daemon whether that should follow the time zone. The Daemon parses it by hand, checking the days of the month and leap years; no date crate is added for twenty lines.
- **`MultiSelect { options, default }`**, submitted as the list of the chosen options' `value`s.
- **`Description { text }`**, text to read between fields. It has no value: the Daemon drops any value sent for it.

An Argument's `kind` gains `number`. It is filled from what follows the keyword as any Argument is, asked for in a `Number` field, and still reaches `run_action` as a string, since every Argument is one: the number written out one way (`1e3` and `1000.0` both as `1000`, ` 2.50` as `2.5`), however it was filled in, so a shell Extension's arithmetic reads it. The Action panel shows an Action whose number Argument the query fills with something else as a form, since Enter asks for it.

## The checks

On `submit_form`, for an Extension's form View the Daemon knows and for its own Argument form, the Daemon checks each field in the form's order:

- **Required.** `optional` is false by default, so a field is required unless it says otherwise. A blank text (whitespace only), a missing value or an empty multi-select fails with `Required`. A checkbox is never missing, since unticked is a value; otherwise every form with an unticked checkbox would fail. This is a change for existing forms: a text field left blank now fails unless it is marked optional. Nothing is released, and the one example that relied on it, the shell example's timer label, now says `optional`.
- **Number.** A number, or text that reads as one (`25`, ` 2.5 `, `1e3`), finite; within `min` and `max`; on a `step`.
- **Date.** Parses as above, with or without the time as `time` says.
- **Choices.** A select's and each of a multi-select's values are among the options.
- **Pattern.** A text or text area field's whole value matches `pattern` (wrapped as `^(?:…)$`, as HTML's `pattern` is anchored). An optional field left blank skips it. The Daemon already depends on `regex` for Live query triggers, so this adds no dependency. A pattern is compiled when its form is submitted and dropped after; nothing is kept per View. A pattern is checked as it is compiled, wrapped, since a pattern can compile alone and not wrapped (`(?x)\d{6} # note`, whose comment swallows the `)`); one that doesn't compile is dropped as its View arrives, the field taking any text, and the Extension gets a `rejected` saying so, as for a Reserved key. `kotodama extension check` reports the same.
- **Types.** A value of the wrong type (a boolean for a text field) fails with what the field expects.

The checks run on submit only, never per keystroke: a field shown red while the person is still typing tells them nothing they don't know. The keystroke path doesn't change.

## The answer: `ToFrontEnd::FormErrors`

When a field fails, the Extension gets nothing and the form stays open. The Front end that submitted it, and only that one, gets:

```
ToFrontEnd::FormErrors { extension, view, errors: Vec<FieldError { field, message }> }
```

one `FieldError` per wrong field, in the form's order, `field` its id and `message` words for the person, shown under the field: `Required`, `At most 120`, `In steps of 0.5 from 1`, `Not a date: write it as YYYY-MM-DD`, `“nightly” isn't one of the options`, `Not in the form this field takes`.

- **A message of its own, not `ActionFailed`.** `ActionFailed` is one line for the whole menu; a form needs one per field, and the Front end needs to know which form, since a form View is the Extension's and may sit under another.
- **Errors per field, not in the form View.** The Daemon could send the View again with an error on each field, but the View is the Extension's and the Front end holds what was typed; sending it again would race the typing. A separate message leaves the View alone.
- **A message, not a code.** The Daemon's other words for the person (`ActionFailed`, problems) are text, and a Front end shows them as they come. A code per check would let a Front end translate, but nothing in Kotodama is translated yet.
- **Clearing.** A Front end drops a form's errors when it submits the form again, and the Daemon names whatever is still wrong. A right form gets no message: it goes to the Extension, which usually pops or replaces it. So there is no "all clear" message to send on every submit.

An Argument typed after a keyword that isn't a number (`timer soon`) opens the Argument form, as a missing one does, with a `FormErrors` for that field right after the `push_view`, so the person sees why they were asked. The wrong value isn't kept: the field starts empty, and the form's values replace whatever was typed before, so a field cleared in the form is left out rather than given its old value.

Kotodama's own menu draws each field's error under it in a later change; until then its submit hint shows the first wrong field's error, a multi-select shows its chosen options without editing them, a text area, number and date are typed into as a line of text, and a description is left out.

## Cost

Nothing on the keystroke path or in the Index changes. A form is checked once per submit, in time linear in its fields, plus compiling the patterns it has; the compiled regex is freed at once. `ArgumentForm` keeps all of its form's fields, not only its file fields, while it is open, to check the form when it is submitted: a few hundred bytes for the seconds a form is open.
