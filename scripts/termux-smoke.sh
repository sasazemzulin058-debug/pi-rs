#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: scripts/termux-smoke.sh [--binary PATH]

Checks CLI prerequisites and, on Termux, its shell/TMPDIR contract.
This is not an M1a behavior gate: it does not call a provider or fabricate fixture output.
EOF
}

binary="${PI_RS_BINARY:-target/release/pi-rs}"
while (($#)); do
  case "$1" in
    --binary) (($# >= 2)) || { usage; exit 2; }; binary="$2"; shift 2 ;;
    -h|--help) usage >&1; exit 0 ;;
    *) usage; exit 2 ;;
  esac
done

[[ -x "$binary" ]] || { echo "missing executable: $binary" >&2; exit 1; }
help_output=$("$binary" --help)
grep -Eq -- '(^|[[:space:]])-p([,[:space:]]|$)' <<<"$help_output" || { echo "CLI lacks -p print mode" >&2; exit 1; }
grep -Fq -- '--import' <<<"$help_output" || { echo "CLI lacks --import" >&2; exit 1; }
if [[ "${PREFIX:-}" == */com.termux/files/usr ]]; then
  [[ -x "$PREFIX/bin/sh" ]] || { echo "Termux shell missing: $PREFIX/bin/sh" >&2; exit 1; }
  [[ -n "${TMPDIR:-}" && -d "$TMPDIR" && -w "$TMPDIR" ]] || {
    echo "Termux TMPDIR must be writable: ${TMPDIR:-<unset>}" >&2
    exit 1
  }
  [[ "${SHELL:-}" == /* && -x "${SHELL:-}" ]] || { echo "invalid SHELL: ${SHELL:-<unset>}" >&2; exit 1; }
  echo "Termux environment: passed ($PREFIX, $TMPDIR, ${SHELL:-<unset>})"
else
  echo "Termux environment: not evaluated (PREFIX is not a Termux prefix)"
fi

echo "CLI prerequisite smoke: passed ($binary)"
