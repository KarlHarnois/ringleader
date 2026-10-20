# Hint lines

**Every rule in this file binds each change to a TUI key, a dialog's hint line, or the hint bar.**

## Dialog hint lines

**A dialog's hint line lists only two kinds of keys:**

- keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`
- keys that choose from a list, such as `↑ ↓ Ctrl-J Ctrl-K choose`

A hint line never lists a text-editing key: every terminal user already expects those keys. Text-editing keys include cursor movement, `Ctrl-U`, and `Ctrl-W`.

Add a new editing key to `TextField` with its usual terminal meaning, never to a hint.

## The hint bar

**The hint bar is the row of keys that `KeyHints` shows at the bottom of the home screen.** The hint bar lists only keys that pass both tests:

- The key's meaning is Ringleader's own.
- A user presses the key often.

Only the `?` help lists these keys:

- a key that a user presses at most once per session
- a key that a user needs only after a session exits or stops
- a key that repeats work a background job already does
- a second key or a Shift variant for an action that the hint bar lists
