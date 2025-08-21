# Live Action Search and Replace (LASR)

When performing a global search-and-replace using e.g. `sed`, it can be difficult to hone in on the exact correct pattern.
`lasr` makes this easier by showing live results as you type.

![Example of using lasr](lasr.gif)

# Installation

Binary artifacts can be downloaded from the [releases page](https://github.com/rcorre/lasr/releases).

If you have a rust toolchain, you can install from source:

```bash
cargo install lasr
```

# Usage

Simply run `lasr` to start a search-and-replace in the current directory. You may run `lasr <path>` to search a different directory.
This will open a TUI where you can start typing a search pattern and see live matches below.
Once you are happy with the search pattern, press <kbd>Tab</kbd> to start editing the replacement pattern.
Finally,

# Syntax

The pattern syntax is based on the rust [regex](https://docs.rs/regex/latest/regex/#syntax) crate.
The replacement syntax is based on the [replace](https://docs.rs/regex/latest/regex/struct.Regex.html#method.replace) method in that crate.

The following replacements are available:

| Text             | Description               |
| ----             | -----------               |
| `$0`, `${0}`     | Whole match               |
| `$1`, `${1}`     | First capture             |
| `$foo`, `${foo}` | Capture group named "foo" |

# Configuration

The configuration file is located at `$XDG_CONFIG_HOME/lasr/lasr.toml` (`~/.config/lasr/lasr.toml` by default).

# Similar tools

- [sad](https://github.com/ms-jpq/sad) allows you to approve/reject each replacement, but must be re-run each time you change the pattern.
- [sd](https://github.com/chmln/sd) provides a simpler CLI alternative to `sed`, but is not interactive.
