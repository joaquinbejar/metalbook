#!/bin/sh
# Offline test of scripts/series-roll.sh: a fake metalbook-admin, a fake login shell, a fake
# launchctl and plutil, with HOME in a scratch directory, so no RPC, no Hermes request, no
# transaction and no real LaunchAgent is touched. Portable (CI runs it on Linux).
#
#   sh scripts/tests/series-roll.sh
#
# The fake admin appends its arguments, and whether the Hermes key reached it (never the
# value), to $t/calls; it exits with the code in $t/exit if that file exists.

set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
roll=$root/scripts/series-roll.sh
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
mkdir -p "$t/bin" "$t/home"
secret=planted-hermes-key-value-0123456789

cat >"$t/bin/metalbook-admin" <<'EOF'
#!/bin/sh
key=absent
[ -z "${METALBOOK_HERMES_API_KEY:-}" ] || key=present
echo "$* key=$key" >>"$MOCK_DIR/calls"
[ ! -f "$MOCK_DIR/exit" ] || exit "$(cat "$MOCK_DIR/exit")"
echo "xau: dry run, nothing sent (add --execute)"
EOF

# The login shell: runs `-c <script> <name> <args>` with sh, as zsh would.
cat >"$t/bin/fakezsh" <<'EOF'
#!/bin/sh
shift
exec sh -c "$@"
EOF

cat >"$t/bin/launchctl" <<'EOF'
#!/bin/sh
echo "launchctl $*" >>"$MOCK_DIR/launchctl"
case $1 in
print)
	[ "${2#*/*/}" != "$2" ] || exit 0
	case $2 in user/*) exit 1 ;; esac
	if [ -f "$MOCK_DIR/loaded" ]; then
		printf '\tstate = not running\n\truns = 3\n\tlast exit code = 0\n'
		exit 0
	fi
	exit 1
	;;
bootstrap) touch "$MOCK_DIR/loaded" ;;
bootout) rm -f "$MOCK_DIR/loaded" ;;
esac
EOF
printf '#!/bin/sh\nexit 0\n' >"$t/bin/plutil"
chmod +x "$t/bin/metalbook-admin" "$t/bin/fakezsh" "$t/bin/launchctl" "$t/bin/plutil"

# The key lives only in the login shell's rc file.
printf 'export METALBOOK_HERMES_API_KEY=%s\n' "$secret" >"$t/home/.zshrc"

fail=0
check() {
	if [ "$2" = "$3" ]; then
		echo "ok: $1"
	else
		echo "FAIL: $1: expected [$3], got [$2]"
		fail=1
	fi
}

# Runs series-roll.sh with $@; prints its exit code; its output goes to $t/out.
sr() {
	set +e
	env -i PATH="$t/bin:/usr/bin:/bin" HOME="$t/home" MOCK_DIR="$t" \
		SOLANA_URL=https://api.devnet.solana.com METALBOOK_PROGRAM_ID=P \
		METALBOOK_KEYPAIR=/keys/lister.json METALBOOK_ROLL_ARGS="--min-expiry 1795208400" \
		METALBOOK_ADMIN_BIN="$t/bin/metalbook-admin" METALBOOK_ROLL_SHELL=fakezsh \
		METALBOOK_ROLL_LOG_MAX="${LOG_MAX:-10485760}" METALBOOK_ROLL_DRY_RUN="${DRY:-0}" \
		${PLANT:+METALBOOK_HERMES_API_KEY=$secret} \
		sh "$roll" "$@" >"$t/out" 2>&1
	echo $?
	set -e
}

calls() { tr '\n' ';' <"$t/calls"; }
dir=$t/home/.local/share/metalbook-roller/r1
plist=$t/home/Library/LaunchAgents/com.metalbook.roll.r1.plist

# install: the binary and the script are copied, one dry run, the agent loaded. A key planted
# in the installing environment never reaches the plist.
: >"$t/calls"
check "install: exit" "$(PLANT=1 sr install r1)" 0
check "install: one dry run, key from the rc file" "$(calls)" "roll --min-expiry 1795208400 key=present;"
check "install: binary copied" "$(test -x "$dir/bin/metalbook-admin" && echo yes)" yes
check "install: script copied" "$(test -f "$dir/series-roll.sh" && echo yes)" yes
check "install: plist written" "$(test -f "$plist" && echo yes)" yes
check "install: no secret in the plist" "$(grep -c "$secret" "$plist" || true)" 0
check "install: no key variable in the plist" "$(grep -c HERMES "$plist" || true)" 0
check "install: runs the copied script" "$(grep -c "$dir/series-roll.sh" "$plist")" 1
check "install: every 6 hours" "$(grep -c '<integer>21600</integer>' "$plist")" 1
check "install: loaded" "$(test -f "$t/loaded" && echo yes)" yes

# run: --execute, the key from the rc file only; a second run is the same call (the binary
# keeps it idempotent) and exits 0.
: >"$t/calls"
check "run: exit" "$(sr run r1)" 0
check "run: rerun exit" "$(sr run r1)" 0
check "run: executes twice" "$(calls)" \
	"roll --min-expiry 1795208400 --execute key=present;roll --min-expiry 1795208400 --execute key=present;"
check "run: the key value is never logged" "$(grep -c "$secret" "$t/out" || true)" 0

# A dry run omits --execute.
: >"$t/calls"
check "dry run: exit" "$(DRY=1 sr run r1)" 0
check "dry run: no --execute" "$(calls)" "roll --min-expiry 1795208400 key=present;"

# A failing roll fails the run with its exit code, and says the next run retries.
echo 3 >"$t/exit"
check "failure: exit code" "$(sr run r1)" 3
check "failure: logged" "$(grep -c 'roll failed (exit 3)' "$t/out")" 1
rm -f "$t/exit"

# Rotation: over the limit, copied to roll.log.1 and truncated; 5 copies kept.
for i in 1 2 3 4 5 6; do
	printf '%0200d\n' "$i" >"$dir/roll.log"
	check "rotation $i: exit" "$(LOG_MAX=100 sr run r1)" 0
done
check "rotation: truncated" "$(wc -c <"$dir/roll.log" | tr -d ' ')" 0
check "rotation: newest copy" "$(cut -c1-200 "$dir/roll.log.1" | tr -d 0)" 6
check "rotation: five kept" "$(ls "$dir" | grep -c '^roll\.log\.')" 5
check "rotation: oldest kept is the second" "$(cut -c1-200 "$dir/roll.log.5" | tr -d 0)" 2
printf 'small\n' >"$dir/roll.log"
sr run r1 >/dev/null
check "rotation: under the limit, untouched" "$(head -1 "$dir/roll.log")" small

# status reports the agent; uninstall unloads it and removes the plist.
check "status: exit" "$(sr status r1)" 0
check "status: state" "$(grep -c 'state = not running' "$t/out")" 1
check "uninstall: exit" "$(sr uninstall r1)" 0
check "uninstall: plist removed" "$(test -f "$plist" && echo yes || echo no)" no
sr status r1 >/dev/null
check "status after uninstall" "$(grep -c 'not loaded' "$t/out")" 1

# Bad names and usage.
check "bad name" "$(sr run 'a b')" 2
check "usage" "$(sr run)" 2

exit $fail
