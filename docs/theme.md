# Theme

**To change Ringleader's colors, write a `theme.toml` file next to the config.** By default, the file is `~/.config/ringleader/theme.toml`. If you pass `--config <path>`, the file goes in the same directory as that config. Ringleader reads the file at startup and when you press `r`.

```toml
selected = { bg = "#333333", bold = true }
dim = { fg = 240 }

[state]
exited = { fg = "red" }
```

A dotted role goes under its table: `state.exited` is `exited` under `[state]`.

A role that the file leaves out keeps its default style. Every default style uses your terminal's palette. If a left-out role has a Follows entry, the role starts from the style of the role it follows instead.

| Role | Follows | What it colors |
|---|---|---|
| `base` |  | all text and the background |
| `selected` |  | the cursor row and the focused field |
| `selected_out_of_focus` |  | the cursor row while the terminal lacks focus |
| `selected_text` |  | selected text in a prompt |
| `prompt` |  | the text you type in a prompt or a name field, and the prompt box's cursor |
| `accent` |  | hot keys in the session menu |
| `dim` |  | secondary text |
| `emphasis` |  | names and headings |
| `matched` | `emphasis` | the letters a search matched |
| `error` |  | errors |
| `warning` |  | warnings |
| `diff.insertions` |  | added-line counts |
| `diff.deletions` |  | removed-line counts |
| `border` |  | the borders of the session list and the preview |
| `title` |  | panel and dialog titles |
| `dialog.border` | `border` | the borders of dialogs and the session menu |
| `separator` | `dim` | rule lines |
| `table.header` | `emphasis` | the session list's column header |
| `repo.name` | `emphasis` | repo names |
| `session.branch_prefix` | `dim` | the branch prefix before a session name |
| `session.agent` | `dim` | the agent column |
| `hint` | `dim` | hint lines in dialogs |
| `hint_bar.key` |  | keys in the hint bar |
| `hint_bar.text` |  | actions in the hint bar |
| `status.error` | `error` | errors in the status bar |
| `state.working` |  | a working session |
| `state.waiting` |  | a session that waits for you |
| `state.exited` |  | a session whose agent exited |
| `state.stopped` | `dim` | a stopped session |
| `state.unmanaged` | `warning` | a tmux session that Ringleader did not create |
| `pull_request.open` |  | an open pull request |
| `pull_request.draft` | `warning` | a draft pull request |
| `pull_request.merged` |  | a merged pull request |
| `pull_request.closed` |  | a closed pull request |
| `pull_request.approved` | `pull_request.open` | an open pull request with an approving review |

A role's style replaces the role's whole default. A style takes `fg` and `bg`, each a color. A style also takes these modifiers, each `true` or `false`:

- `bold`
- `dim`
- `italic`
- `underlined`
- `reversed`
- `crossed_out`

A color takes one of these forms:

- `default`, which keeps the terminal's color
- an ANSI name, such as `red` or `bright_cyan`
- a number from 0 to 255
- `#rrggbb`

If the `NO_COLOR` variable is set and not empty, Ringleader ignores `theme.toml` and shows the default modifiers without color.
