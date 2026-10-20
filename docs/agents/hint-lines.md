# Hint lines

**Every rule in this file binds each change to a TUI key, a dialog's hint line, or the hint bar.**

## Dialog hint lines

**A dialog's hint line lists only two kinds of keys:**

- keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`
- keys that choose from a list, such as `↑ ↓ Ctrl-J Ctrl-K choose`

A hint line never lists a text-editing key: every terminal user already expects those keys. Text-editing keys include cursor movement, `Ctrl-U`, and `Ctrl-W`.

## The hint bar

**Besides `?` and `q`, every key that `KeyHints` lists passes both tests:**

- The key's meaning is Ringleader's own.
- A user presses the key often.
