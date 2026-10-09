# tt hooks

Hooks are executables the daemon runs after each event (see
[events.md](events.md)). They live in `$XDG_CONFIG_HOME/tt/hooks/`
(usually `~/.config/tt/hooks/`):

```
hooks/
├── entry.started          # runs for entry.started
├── entry.stopped.d/       # every executable inside runs for entry.stopped
│   ├── 10-notify
│   └── 20-log
└── all.d/                 # runs for every event
    └── sync-statusbar
```

For each event the daemon runs `hooks/<type>`, then `hooks/<type>.d/*`, then
`hooks/all.d/*`, each directory in name order. Files without an executable
bit are skipped. Hooks are discovered on every event, so adding or removing
one needs no restart.

## Contract

- **stdin**: the event JSON (same as a `tt watch` line), followed by a newline.
- **environment**: `TT_EVENT` (type, e.g. `entry.started`), `TT_ORIGIN`
  (`local` or `remote`), `TT_SEQ` (event seq), plus the daemon's environment.
- **timeout**: 10 seconds; then the hook is killed.
- **output**: stdout and stderr are written to the daemon log
  (`$XDG_STATE_HOME/tt/daemon.log`). Non-zero exits and timeouts are logged
  as warnings.
- **never blocking**: hooks run after the change is committed. The command
  that caused the event has already returned. A slow hook delays only the
  later hooks of the same event.
- **renumbering**: when two devices created the same `#seq` offline, the
  repair after sync fires `task.renumbered` (stdin carries `from` and `to`)
  rather than `task.updated`; a hook keyed on task numbers should listen for it.
- **filtering is the hook's job**: use `TT_EVENT`, `TT_ORIGIN`, or `jq` on stdin.
  Remote changes (from another device) fire hooks too; check
  `TT_ORIGIN=local` when a hook should only react to this machine.

## Examples

Desktop notification when a timer starts on this machine:

```sh
#!/bin/sh
# ~/.config/tt/hooks/entry.started
[ "$TT_ORIGIN" = local ] || exit 0
title=$(jq -r '"#\(.task.seq) \(.task.title)"')
notify-send "tt: started" "$title"
```

Append every event to a file:

```sh
#!/bin/sh
# ~/.config/tt/hooks/all.d/log
cat >> "$HOME/.local/state/tt/events.ndjson"
```

## Quickshell status widget

Hooks suit one-off reactions. For a live status bar prefer `tt watch`: it
starts with a snapshot of running entries and pushes every change, with no
polling. Quickshell runs it as a `Process` and parses each line:

```qml
// RunningTimers.qml
import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root
    property var running: []        // EntryView objects from the daemon
    property int lastSeq: 0

    Process {
        id: watcher
        command: root.lastSeq > 0
            ? ["tt", "watch", "--event", "entry.*", "--since", String(root.lastSeq)]
            : ["tt", "watch", "--event", "entry.*"]
        running: true
        stdout: SplitParser {
            onRead: line => {
                const event = JSON.parse(line)
                if (event.seq !== undefined) root.lastSeq = event.seq
                // snapshot and every entry.* event carry the full running list
                if (event.running !== undefined) root.running = event.running
            }
        }
        // Daemon restarted or stream ended: reconnect after a second.
        onExited: restart.start()
    }

    Timer {
        id: restart
        interval: 1000
        onTriggered: watcher.running = true
    }

    function label() {
        if (root.running.length === 0) return "idle"
        return root.running
            .map(e => `#${e.task.seq} ${e.task.title}`)
            .join("  ·  ")
    }
}
```

Elapsed time is `now - entry.start`; update it with a 1 second `Timer`
locally instead of asking the daemon.
