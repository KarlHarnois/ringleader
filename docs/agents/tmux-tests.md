# tmux tests

**Every rule in this file binds each test that runs tmux.**

- tmux tests never skip.
- Each tmux test starts its own server through `TmuxTestServer`, on a private socket under `/tmp`.
- The test server loads the rendered generated config. If the test is about the config itself, the test server loads a fixture config instead.
- Each tmux test builds its snapshot from the test process's own `PATH`.
- A test that needs a client holds a control-mode client.
- Press keys through the control-mode client with `send-keys -K -c <client>`. Never press keys with `send-keys -t <pane>`, which skips the key tables.
- Only a test of a nested client's own parsing MAY type raw terminal bytes with `send-keys -t <pane> -H`. That test types into a pane of a second test server that holds the nested client.
