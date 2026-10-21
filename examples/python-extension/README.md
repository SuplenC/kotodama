# A Kotodama Extension in Python

`extension.py` is a whole Extension in Python 3.8 or later, with the standard library alone and JSON as its Wire format. [docs/extensions.md](../../docs/extensions.md) is the guide to what it does; this is the shortest way to see it work.

Type `case hello world` and the menu lists the text in each case: `HELLO WORLD`, `hello world`, `Hello World`, `hello_world`, `hello-world` and `helloWorld`. Enter copies the one you pick; Ctrl+T types it into the window you came from. `case` and Tab enters its app, where everything you type is converted.

It shows:

- the handshake, offering JSON alone, and turning away a Daemon older than the protocol it was written for;
- a Live query: the Manifest's `[live_query]` prefix `case ` routes queries to it, and it answers each with `live_results`, best first, then `done`;
- two Actions on every Result, the first the primary one, the second bound to Ctrl+T, answered with `copy` and `type` outcomes, which the Daemon carries out;
- its config table, `[extensions.text-case] cases`, which picks the cases it offers;
- `status`: with no case it knows, it tells the Daemon it can't do anything, and why, so the search leaves its app out;
- logging `rejected`, the Daemon's reason for refusing a message it sent, to stderr.

## Trying it

```sh
kotodama extension check examples/python-extension
ln -s "$PWD/examples/python-extension" ~/.config/kotodama/extensions/text-case
```

The Daemon picks it up without a restart. To offer fewer cases, in any file in `~/.config/kotodama/`:

```toml
[extensions.text-case]
cases = ["snake", "kebab", "camel"]
```

`crates/kotodama-protocol/tests/python_extension.rs` runs it as the Daemon does.
