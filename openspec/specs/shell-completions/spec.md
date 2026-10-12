# shell-completions Specification

## Purpose
Shell completion for the `tt` and `tt-server` CLIs: completion script generation for `tt` and `tt-server`, value completion for enumerated arguments, file and directory hints for path arguments, and packaging of the zsh scripts so completion works out of the box after install.

## Requirements

### Requirement: Completion script generation
`tt completions <shell>` and `tt-server completions <shell>` SHALL print to stdout a completion script for the named shell (at least `zsh`, `bash`, `fish`), covering every subcommand, flag and enumerated value of that binary at the time it was built. The subcommand SHALL be hidden from `--help`, SHALL NOT start the daemon or open the database, and SHALL exit 0. An unknown shell SHALL be a usage error (exit code 1).

#### Scenario: Zsh script for tt
- **WHEN** `tt completions zsh` is run with no daemon running
- **THEN** stdout is a zsh completion script defining `_tt`, no daemon is spawned, and the exit code is 0

#### Scenario: Zsh script for tt-server
- **WHEN** `tt-server completions zsh` is run without a database
- **THEN** stdout is a zsh completion script defining `_tt-server` and no database file is created

#### Scenario: Unknown shell
- **WHEN** `tt completions tcsh` is run
- **THEN** stdout is empty and the exit code is 1

### Requirement: Enumerated arguments complete and validate
The following arguments SHALL accept only their documented values, offer them to shell completion, and reject anything else as a usage error (exit code 1): `task ls --state` and `task mod --state` and `task find --state` (`open|done|archived`, plus `all` where documented), `report --by` (`task|tag|project|day`), `export --format` (`json|csv`), `export --mode` (`up|nearest`), `export --group` (`entry|task-day`). Spellings accepted before this change (such as `taskday`) SHALL keep working. `tt config get` and `tt config set` SHALL complete the supported config keys.

#### Scenario: Value completion
- **WHEN** the user completes `tt report --by ` in zsh
- **THEN** `task`, `tag`, `project` and `day` are offered

#### Scenario: Invalid value rejected
- **WHEN** `tt report --by bogus` is run
- **THEN** it exits with code 1 and the message lists the allowed values

#### Scenario: Config keys
- **WHEN** the user completes `tt config set ` in zsh
- **THEN** the supported keys (`server.url`, `tz`, `snap`, ...) are offered

### Requirement: Path arguments complete as files
Arguments that take a file or directory (`tt login --ca-cert`, `tt import <file>`, `tt export -o`, and `tt-server --db`, `serve --web-dir|--tls-cert|--tls-key`) SHALL complete as file or directory names.

#### Scenario: CA file completion
- **WHEN** the user completes `tt login https://x --ca-cert ` in zsh
- **THEN** file names are offered

### Requirement: Packaged zsh completions
The nix package SHALL install `_tt` and `_tt-server` under `share/zsh/site-functions`, generated from the built binaries, so that installing the package enables completion with no manual step.

#### Scenario: Package contents
- **WHEN** the nix package is built
- **THEN** `$out/share/zsh/site-functions/_tt` and `_tt-server` exist and match the output of the corresponding `completions zsh` command
