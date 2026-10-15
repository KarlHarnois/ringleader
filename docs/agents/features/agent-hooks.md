# Agent hooks

**Every rule in this file binds each change to one of these:**

- how `AgentHooks` types into an agent
- the check that an agent is at its prompt

**Send each line's Enter apart from the line's text.** An agent reads text and an Enter that arrive together as a paste. The agent then keeps the Enter in its draft and never submits the line.

**If all three conditions below hold, the agent is at its prompt:**

| Condition | Reason |
| --- | --- |
| The pane is out of copy mode. | Copy mode would take the keys instead of the agent. |
| The pane shows the cursor. | Claude hides the cursor in its trust and permission dialogs. |
| The text before the cursor on its row is a bare prompt marker and a blank. That text holds no letters or digits. | An agent still starting has drawn no marker. An unsent draft puts its text before the cursor. |
