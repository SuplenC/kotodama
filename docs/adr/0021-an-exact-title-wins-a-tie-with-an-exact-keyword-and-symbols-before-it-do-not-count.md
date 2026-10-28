# 21. An exact title wins a tie with an exact keyword, and symbols before a title don't count

Status: accepted (2026-10-20); amended by ADR 0034 (only an Item's own keyword matches as exact; its search words match as a word start)

Typing `fire` put 🔥 fifth, behind 🧑‍🚒 Firefighter and 🧨 Firecracker. The emoji Extension titles each emoji with the character first, `🔥 Fire`, so the title was never exactly the query, while CLDR gives the firefighter and the firecracker the keyword `fire`, an exact match (ADR 0006). Users expect the thing named as they typed to come first, ahead of things that only list the word among their keywords.

Two changes, both inside the exact Tier:

- **Symbols before a title don't count.** A title is exact when it is the query, or when what follows the symbols that lead it, and the space after them, is: `🔥 Fire` for `fire`. A symbol is anything but a letter, a digit or a space, so `~/fire` and `Fire 🔥` aren't exact for `fire`.
- **An exact title wins a tie with an exact keyword.** Within a Tier, Results rank by Frecency, then an exact title over an exact keyword, as initials over a word start (ADR 0011), then the rest of ADR 0011's order. Both still rank as exact: a keyword never drops below a word start, and any use beats the difference.

An app titled `Fire` and the emoji `🔥 Fire` are both exact titles, so the Item kind still puts the app first (ADR 0011).

## Considered options

- **The emoji's name as one of its keywords.** Rejected alone: it ties with every emoji that has the keyword, and the shorter title then decides. That ranked 🦆 Duck above 🐦 Bird for `bird`, and failed the same way for a third of the names that are another emoji's keyword.
- **The character in the icon rather than the title.** Rejected: an icon is a freedesktop icon name or a path, and the Front end would need a new way to draw a character.
- **Skipping any leading punctuation, without the space.** Rejected: `.config` or `~/notes` would turn exact for `config` and `notes`, which nobody asked for.
