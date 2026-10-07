# tt command line reference

Generated from the clap definitions with `tt __docs-cli > docs/cli.md`; do not edit by hand.

Exit codes: 0 ok, 1 usage, 2 not found, 3 daemon unreachable, 4 invalid state or conflict.
Every read command accepts `-j` for JSON (uuids, seqs, UTC timestamps, durations in seconds).

## `tt`

```text
Task and time tracker: offline-first, concurrent timers, hooks and an event stream

Usage: tt [OPTIONS] <COMMAND>

Commands:
  task     Manage tasks
  project  Manage projects
  tag      Manage tags
  start    Start a timer: by #seq/uuid, fuzzy text, or (on a TTY, no argument) a picker
  stop     Stop the running entry; with several running pass an entry or task id, or --all
  time     Time entries
  report   Summaries with summed and wall-clock totals
  export   Export data as JSON or CSV, optionally rounded and flattened
  import   Import a JSON export, matching records by uuid
  watch    Stream events as NDJSON (first line: snapshot of running entries)
  status   Daemon, sync and running entries
  daemon   Run the daemon in the foreground
  login    Log in to a tt server and enable sync
  config   Read or change config.toml
  help     Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task`

```text
Manage tasks

Usage: tt task [OPTIONS] <COMMAND>

Commands:
  add   Create: tt task add "Title" +tag @project key:value
  ls    List tasks (default: open)
  show  Show one task with its entries
  edit  Edit in $EDITOR as markdown with YAML frontmatter
  mod   Modify fields
  done  Mark done
  rm    Delete (refuses when entries exist unless --force)
  find  Fuzzy search title, tags and metadata values
  help  Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task add`

```text
Create: tt task add "Title" +tag @project key:value

Usage: tt task add [OPTIONS] <ARGS>...

Arguments:
  <ARGS>...
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task ls`

```text
List tasks (default: open)

Usage: tt task ls [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --state <STATE>
          open|done|archived|all

      --tag <TAG>
          

      --project <PROJECT>
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task show`

```text
Show one task with its entries

Usage: tt task show [OPTIONS] <TASK>

Arguments:
  <TASK>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task edit`

```text
Edit in $EDITOR as markdown with YAML frontmatter

Usage: tt task edit [OPTIONS] <TASK>

Arguments:
  <TASK>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task mod`

```text
Modify fields

Usage: tt task mod [OPTIONS] <TASK>

Arguments:
  <TASK>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --title <TITLE>
          

      --description <DESCRIPTION>
          

      --add-tag <ADD_TAG>
          

      --rm-tag <RM_TAG>
          

      --project <PROJECT>
          

      --no-project
          

      --meta <META>
          key=value (repeatable)

      --rm-meta <RM_META>
          

      --state <STATE>
          open|done|archived

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task done`

```text
Mark done

Usage: tt task done [OPTIONS] <TASK>

Arguments:
  <TASK>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task rm`

```text
Delete (refuses when entries exist unless --force)

Usage: tt task rm [OPTIONS] <TASK>

Arguments:
  <TASK>
          

Options:
      --force
          

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt task find`

```text
Fuzzy search title, tags and metadata values

Usage: tt task find [OPTIONS] <QUERY>...

Arguments:
  <QUERY>...
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --state <STATE>
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt project`

```text
Manage projects

Usage: tt project [OPTIONS] <COMMAND>

Commands:
  add   
  ls    
  mod   Rename, recolor, (un)archive
  rm    
  help  Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt project add`

```text
Usage: tt project add [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
      --color <COLOR>
          

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt project ls`

```text
Usage: tt project ls [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt project mod`

```text
Rename, recolor, (un)archive

Usage: tt project mod [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --name <NEW_NAME>
          

      --color <COLOR>
          

      --archive
          

      --unarchive
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt project rm`

```text
Usage: tt project rm [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt tag`

```text
Manage tags

Usage: tt tag [OPTIONS] <COMMAND>

Commands:
  add   
  ls    
  mod   Rename, recolor, (un)archive
  rm    
  help  Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt tag add`

```text
Usage: tt tag add [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
      --color <COLOR>
          

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt tag ls`

```text
Usage: tt tag ls [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt tag mod`

```text
Rename, recolor, (un)archive

Usage: tt tag mod [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --name <NEW_NAME>
          

      --color <COLOR>
          

      --archive
          

      --unarchive
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt tag rm`

```text
Usage: tt tag rm [OPTIONS] <NAME>

Arguments:
  <NAME>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt start`

```text
Start a timer: by #seq/uuid, fuzzy text, or (on a TTY, no argument) a picker

Usage: tt start [OPTIONS] [TASK]...

Arguments:
  [TASK]...
          Task id (#12, 12, uuid) or text to fuzzy-match against open tasks

Options:
      --at <AT>
          Start time (13:00, -15m, yesterday 9:00, ISO); default now

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --note <NOTE>
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt stop`

```text
Stop the running entry; with several running pass an entry or task id, or --all

Usage: tt stop [OPTIONS] [TARGET]

Arguments:
  [TARGET]
          Entry id (prefix) or task id

Options:
      --all
          

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --at <AT>
          Stop time; default now

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time`

```text
Time entries

Usage: tt time [OPTIONS] <COMMAND>

Commands:
  ls     List entries (default range: today)
  mod    Change start, end, task or note
  move   Move to another task
  split  Split into two adjacent entries
  rm     Delete
  edit   Edit entries as a table in $EDITOR (default range: today)
  help   Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time ls`

```text
List entries (default range: today)

Usage: tt time ls [OPTIONS] [RANGE]

Arguments:
  [RANGE]
          [default: today]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time mod`

```text
Change start, end, task or note

Usage: tt time mod [OPTIONS] <ENTRY>

Arguments:
  <ENTRY>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --start <START>
          

      --end <END>
          End time, or "running"

      --task <TASK>
          

      --note <NOTE>
          Note (empty string clears)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time move`

```text
Move to another task

Usage: tt time move [OPTIONS] --task <TASK> <ENTRY>

Arguments:
  <ENTRY>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --task <TASK>
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time split`

```text
Split into two adjacent entries

Usage: tt time split [OPTIONS] --at <AT> <ENTRY>

Arguments:
  <ENTRY>
          

Options:
      --at <AT>
          

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time rm`

```text
Delete

Usage: tt time rm [OPTIONS] <ENTRY>

Arguments:
  <ENTRY>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt time edit`

```text
Edit entries as a table in $EDITOR (default range: today)

Usage: tt time edit [OPTIONS] [RANGE]

Arguments:
  [RANGE]
          [default: today]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt report`

```text
Summaries with summed and wall-clock totals

Usage: tt report [OPTIONS] [RANGE]

Arguments:
  [RANGE]
          today|yesterday|week|lastweek|month|lastmonth|year|all|YYYY-MM-DD|<from>..<to>
          
          [default: week]

Options:
      --by <BY>
          task|tag|project|day
          
          [default: task]

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt export`

```text
Export data as JSON or CSV, optionally rounded and flattened

Usage: tt export [OPTIONS] [RANGE]

Arguments:
  [RANGE]
          Range (default: everything)

Options:
      --format <FORMAT>
          json|csv
          
          [default: json]

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --round <ROUND>
          Rounding grid, e.g. 15m (default: config `snap` when --mode/--group given)

      --mode <MODE>
          up|nearest

      --group <GROUP>
          entry|task-day

  -o, --output <OUTPUT>
          Write to a file instead of stdout

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt import`

```text
Import a JSON export, matching records by uuid

Usage: tt import [OPTIONS] <FILE>

Arguments:
  <FILE>
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt watch`

```text
Stream events as NDJSON (first line: snapshot of running entries)

Usage: tt watch [OPTIONS]

Options:
      --event <EVENTS>
          Event type, e.g. entry.started or entry.* (repeatable)

  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --task <TASKS>
          

      --tag <TAGS>
          

      --project <PROJECTS>
          

      --since <SINCE>
          Replay events after this seq

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt status`

```text
Daemon, sync and running entries

Usage: tt status [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt daemon`

```text
Run the daemon in the foreground

Usage: tt daemon [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt login`

```text
Log in to a tt server and enable sync

Usage: tt login [OPTIONS] <URL>

Arguments:
  <URL>
          Server base URL, e.g. https://tt.example.com

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

      --username <USERNAME>
          

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt config`

```text
Read or change config.toml

Usage: tt config [OPTIONS] <COMMAND>

Commands:
  get   Print one key, or the whole file
  set   Set a key; omit the value to unset
  path  Print the config file path
  help  Print this message or the help of the given subcommand(s)

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt config get`

```text
Print one key, or the whole file

Usage: tt config get [OPTIONS] [KEY]

Arguments:
  [KEY]
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt config set`

```text
Set a key; omit the value to unset

Usage: tt config set [OPTIONS] <KEY> [VALUE]

Arguments:
  <KEY>
          

  [VALUE]
          

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

## `tt config path`

```text
Print the config file path

Usage: tt config path [OPTIONS]

Options:
  -j, --json
          JSON output (stable schema: uuids, seqs, UTC timestamps, durations in seconds)

  -h, --help
          Print help

  -V, --version
          Print version
```

