# uml-viewer

**This file is the procedure for opening uml-viewer on the metrics snapshots.**

uml-viewer reads `.metrics/crap.edn` and `.metrics/mutate/`. uml-viewer ignores `.metrics/dry.edn`, so `dryer` output stays a text report and never reaches the diagram. uml-viewer needs a display, and downloads Maven dependencies on its first run.

Write this policy to the gitignored `.uml-viewer/ringleader.policy.edn`:

```edn
{:title "Ringleader"
 :sources [{:lang :rust :root "src" :prefix "ringleader"}]
 :hierarchical true
 :out ".uml-viewer/ringleader.edn"}
```

**Leave the policy-level `:prefix` unset.** The overlay joins a `crate::module::Type` snapshot to the module class through the class id's crate path. Stripping the crate name from the id breaks that join.

The `metrics` shell sets `$UML_VIEWER_DEPS`, which points the Clojure CLI at the `uml-viewer` flake input. Inside `nix develop .#metrics`, generate the IR, then open the viewer:

```sh
clojure -Sdeps "$UML_VIEWER_DEPS" -M:ir .uml-viewer/ringleader.policy.edn
viewer_tmux_dir=$(mktemp -d /tmp/uml.XXXXXX)
env -u TMUX -u TMUX_PANE TMUX_TMPDIR="$viewer_tmux_dir" \
  clojure -Sdeps "$UML_VIEWER_DEPS" -M:run --restart .uml-viewer/ringleader.edn
rm -rf "$viewer_tmux_dir"
```

**Always pass `--restart`.** Without `--restart`, the viewer starts `grok --yolo --trust` as its companion, in a new session on the project owner's tmux server. The viewer then opens on a blank canvas until that companion answers. `--restart` skips the companion and loads the EDN at once.

The viewer still looks for a companion with `tmux has-session`. The `env` prefix sends those lookups to an empty private directory, which the commands remove after the viewer exits.
