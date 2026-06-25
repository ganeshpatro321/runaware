use crate::cli::ShellKind;

pub fn init_script(shell: ShellKind) -> &'static str {
    match shell {
        ShellKind::Zsh | ShellKind::Bash => POSIX_SCRIPT,
        ShellKind::Fish => FISH_SCRIPT,
    }
}

const POSIX_SCRIPT: &str = r#"# RunAware shell integration
# Install for this shell session:
#   eval "$(runaware shell init zsh)"
#
# Optional per-command override:
#   RUNAWARE_SOURCE=api npm run dev

__runaware_capture() {
  local __runaware_cmd="$1"
  shift
  if [ -n "${RUNAWARE_CAPTURE_ACTIVE:-}" ] && [ -z "${RUNAWARE_ALLOW_NESTED:-}" ]; then
    command "$__runaware_cmd" "$@"
    return $?
  fi
  case "$1" in
    -v|--version|version|-h|--help|help)
      command "$__runaware_cmd" "$@"
      return $?
      ;;
  esac
  if ! command -v runaware >/dev/null 2>&1; then
    command "$__runaware_cmd" "$@"
    return $?
  fi
  command runaware capture --source auto -- "$__runaware_cmd" "$@"
}

npm() { __runaware_capture npm "$@"; }
pnpm() { __runaware_capture pnpm "$@"; }
yarn() { __runaware_capture yarn "$@"; }
bun() { __runaware_capture bun "$@"; }
node() { __runaware_capture node "$@"; }
python() { __runaware_capture python "$@"; }
python3() { __runaware_capture python3 "$@"; }
pytest() { __runaware_capture pytest "$@"; }
go() { __runaware_capture go "$@"; }
cargo() { __runaware_capture cargo "$@"; }

docker() {
  if [ -n "${RUNAWARE_CAPTURE_ACTIVE:-}" ] && [ -z "${RUNAWARE_ALLOW_NESTED:-}" ]; then
    command docker "$@"
    return $?
  fi
  if [ "$1" = "compose" ] || [ "$1" = "logs" ]; then
    if command -v runaware >/dev/null 2>&1; then
      RUNAWARE_SOURCE="${RUNAWARE_SOURCE:-docker}" command runaware capture --source auto -- docker "$@"
    else
      command docker "$@"
    fi
  else
    command docker "$@"
  fi
}

runaware-checkpoint() {
  command runaware checkpoint "$@"
}
"#;

const FISH_SCRIPT: &str = r#"# RunAware shell integration for fish
function __runaware_capture
  set cmd $argv[1]
  set -e argv[1]
  if set -q RUNAWARE_CAPTURE_ACTIVE; and not set -q RUNAWARE_ALLOW_NESTED
    command $cmd $argv
    return $status
  end
  switch "$argv[1]"
    case -v --version version -h --help help
      command $cmd $argv
      return $status
  end
  if not command -q runaware
    command $cmd $argv
    return $status
  end
  command runaware capture --source auto -- $cmd $argv
end

function npm; __runaware_capture npm $argv; end
function pnpm; __runaware_capture pnpm $argv; end
function yarn; __runaware_capture yarn $argv; end
function bun; __runaware_capture bun $argv; end
function node; __runaware_capture node $argv; end
function python; __runaware_capture python $argv; end
function python3; __runaware_capture python3 $argv; end
function pytest; __runaware_capture pytest $argv; end
function go; __runaware_capture go $argv; end
function cargo; __runaware_capture cargo $argv; end
function docker
  if set -q RUNAWARE_CAPTURE_ACTIVE; and not set -q RUNAWARE_ALLOW_NESTED
    command docker $argv
    return $status
  end
  if test "$argv[1]" = "compose"; or test "$argv[1]" = "logs"
    if command -q runaware
      set -lx RUNAWARE_SOURCE docker
      command runaware capture --source auto -- docker $argv
    else
      command docker $argv
    end
  else
    command docker $argv
  end
end

function runaware-checkpoint
  command runaware checkpoint $argv
end
"#;

#[cfg(test)]
mod tests {
    use super::init_script;
    use crate::cli::ShellKind;

    #[test]
    fn shell_integration_skips_nested_capture_by_default() {
        let zsh = init_script(ShellKind::Zsh);
        assert!(zsh.contains("RUNAWARE_CAPTURE_ACTIVE"));
        assert!(zsh.contains("RUNAWARE_ALLOW_NESTED"));
        assert!(zsh.contains("command \"$__runaware_cmd\" \"$@\""));

        let fish = init_script(ShellKind::Fish);
        assert!(fish.contains("RUNAWARE_CAPTURE_ACTIVE"));
        assert!(fish.contains("RUNAWARE_ALLOW_NESTED"));
        assert!(fish.contains("command $cmd $argv"));
    }
}
