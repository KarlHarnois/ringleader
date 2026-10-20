# Hint lines

**Every rule in this file binds each change to a dialog's hint line and each new text-editing key.**

**A dialog's hint line lists only two kinds of keys:**

- keys whose meaning is Ringleader's own, such as `Enter create`, `Ctrl-S create`, and `Esc cancel`
- keys that choose from a list, such as `↑ ↓ Ctrl-J Ctrl-K choose`

**Never list a text-editing key in a hint line:** every terminal user already expects those keys. A text-editing key is a key that moves the cursor in a text field or edits the field's text.

**Add each new text-editing key to `TextField`, with the meaning the key has in readline.**
