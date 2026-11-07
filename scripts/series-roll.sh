#!/bin/sh
# The weekly series roller as a LaunchAgent (docs/PLAN-phase2.md, R1; docs/runbooks/series-roll.md).
#
#   scripts/series-roll.sh run <name>
#   scripts/series-roll.sh install <name>
#   scripts/series-roll.sh uninstall <name>
#   scripts/series-roll.sh status <name>
#
# `run` rotates the log, then runs `metalbook-admin roll --execute` once (the decisions, the
# bounds and the idempotence are in the binary: a second run sends nothing). The Hermes key
# comes from the login shell's ~/.zshrc (`zsh -c 'source ~/.zshrc; exec ...'`), never from the
# plist, a file of this agent, or an argument. With METALBOOK_ROLL_DRY_RUN=1 it runs without
# `--execute`: the plan only.
#
# `install` copies this script and the metalbook-admin binary (default this checkout's
# target/release/metalbook-admin; `cargo build --release -p metalbook-cli` first) into
# $METALBOOK_ROLLER_HOME/<name>/ (default ~/.local/share/metalbook-roller), so rebuilding the
# checkout never changes a running agent, runs one dry run, and writes the LaunchAgent
# com.metalbook.roll.<name>: `run` at load and every 6 hours, logging to
# $METALBOOK_ROLLER_HOME/<name>/roll.log. Log rotation: at the start of every `run`, a log over
# 10 MB is copied to roll.log.1 (older copies shifted, 5 kept) and truncated.
#
# Configuration, from the environment and written into the plist, so none of it may hold a
# secret (a public RPC URL; the keypair by path):
#   SOLANA_URL, METALBOOK_PROGRAM_ID  as for metalbook-admin
#   METALBOOK_KEYPAIR                 the lister key (pays the series' rent; docs/keys.md)
#   METALBOOK_ROLL_ARGS               extra `roll` options, e.g. `--min-expiry 1795208400`
#   METALBOOK_ADMIN_BIN               the binary to copy at install (default target/release/)
#
# For the offline test (scripts/tests/series-roll.sh) only, never written into the plist:
#   METALBOOK_ROLL_SHELL              the login shell (default zsh)
#   METALBOOK_ROLL_LOG_MAX            the rotation size in bytes (default 10485760)

set -eu

usage() {
	sed -n '4,7p' "$0" | sed 's/^# *//' >&2
	exit 2
}

[ $# -eq 2 ] || usage
cmd=$1
name=$2
case $name in
*[!A-Za-z0-9_-]* | '') echo "name must be letters, digits, - or _" >&2 && exit 2 ;;
esac
root=$(cd "$(dirname "$0")/.." && pwd)
home=${METALBOOK_ROLLER_HOME:-$HOME/.local/share/metalbook-roller}
dir=$home/$name
label=com.metalbook.roll.$name
plist=$HOME/Library/LaunchAgents/$label.plist

log() { echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) $*"; }

# Copy and truncate roll.log when it is over the limit, keeping roll.log.1 to roll.log.5.
rotate() {
	f=$dir/roll.log
	max=${METALBOOK_ROLL_LOG_MAX:-10485760}
	[ -f "$f" ] || return 0
	size=$(wc -c <"$f" | tr -d ' ')
	[ "$size" -gt "$max" ] || return 0
	rm -f "$f.5"
	for i in 4 3 2 1; do
		[ ! -f "$f.$i" ] || mv "$f.$i" "$f.$((i + 1))"
	done
	cp "$f" "$f.1"
	: >"$f"
	log "rotated roll.log ($size bytes) to roll.log.1"
}

run() {
	rotate
	bin=$dir/bin/metalbook-admin
	[ -x "$bin" ] || { log "no $bin: run install first" >&2 && exit 1; }
	execute=--execute
	[ "${METALBOOK_ROLL_DRY_RUN:-0}" = 1 ] && execute=
	log "roll start"
	# The key is set by ~/.zshrc in this child only; nothing here reads or prints it.
	# shellcheck disable=SC2086 # METALBOOK_ROLL_ARGS is word-split on purpose
	if "${METALBOOK_ROLL_SHELL:-zsh}" -c '. "$HOME/.zshrc" >/dev/null 2>&1; exec "$@"' roll \
		"$bin" roll ${METALBOOK_ROLL_ARGS:-} $execute; then
		log "roll done"
	else
		code=$?
		log "roll failed (exit $code); the next run retries"
		exit "$code"
	fi
}

xml() { printf '%s' "$1" | sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g'; }

# One <key>/<string> pair per configuration variable that is set.
env_entries() {
	for v in SOLANA_URL METALBOOK_PROGRAM_ID METALBOOK_KEYPAIR METALBOOK_ROLL_ARGS \
		METALBOOK_ROLLER_HOME; do
		eval "set=\${$v+x} val=\${$v:-}"
		# shellcheck disable=SC2154 # set and val are assigned by the eval above
		if [ -n "$set" ]; then
			printf '\t\t<key>%s</key><string>%s</string>\n' "$v" "$(xml "$val")"
		fi
	done
}

uid=$(id -u)
loaded() {
	for d in "gui/$uid" "user/$uid"; do
		if launchctl print "$d/$label" >/dev/null 2>&1; then
			echo "$d"
		fi
	done
}

unload() {
	for d in $(loaded); do
		if ! launchctl bootout "$d/$label" 2>/dev/null; then
			if launchctl print "$d/$label" >/dev/null 2>&1; then
				echo "could not unload $label from $d" >&2
				exit 1
			fi
		fi
	done
}

case $cmd in
run) run ;;
install)
	: "${SOLANA_URL:?set SOLANA_URL}" "${METALBOOK_PROGRAM_ID:?set METALBOOK_PROGRAM_ID}" \
		"${METALBOOK_KEYPAIR:?set METALBOOK_KEYPAIR}"
	src=${METALBOOK_ADMIN_BIN:-$root/target/release/metalbook-admin}
	[ -x "$src" ] || { echo "no $src: cargo build --release -p metalbook-cli" >&2 && exit 1; }
	mkdir -p "$dir/bin" "$HOME/Library/LaunchAgents"
	cp "$src" "$dir/bin/metalbook-admin.new"
	mv "$dir/bin/metalbook-admin.new" "$dir/bin/metalbook-admin"
	cp "$0" "$dir/series-roll.sh.new"
	mv "$dir/series-roll.sh.new" "$dir/series-roll.sh"
	# A dry run first: it checks the configuration, the RPC and the key before scheduling.
	METALBOOK_ROLL_DRY_RUN=1 sh "$dir/series-roll.sh" run "$name"
	if launchctl print "gui/$uid" >/dev/null 2>&1; then
		domain=gui/$uid
	else
		domain=user/$uid
	fi
	cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key><string>$label</string>
	<key>ProgramArguments</key>
	<array>
		<string>/bin/sh</string>
		<string>$(xml "$dir/series-roll.sh")</string>
		<string>run</string>
		<string>$(xml "$name")</string>
	</array>
	<key>EnvironmentVariables</key>
	<dict>
$(env_entries)
	</dict>
	<key>LimitLoadToSessionType</key>
	<array>
		<string>Aqua</string>
		<string>Background</string>
	</array>
	<key>RunAtLoad</key><true/>
	<key>StartInterval</key><integer>21600</integer>
	<key>StandardOutPath</key><string>$(xml "$dir/roll.log")</string>
	<key>StandardErrorPath</key><string>$(xml "$dir/roll.log")</string>
	<key>WorkingDirectory</key><string>$(xml "$dir")</string>
</dict>
</plist>
EOF
	plutil -lint "$plist" >/dev/null
	unload
	launchctl bootstrap "$domain" "$plist"
	echo "installed $label: $plist"
	echo "log: $dir/roll.log"
	;;
uninstall)
	unload
	rm -f "$plist"
	echo "uninstalled $label (its log and binary stay in $dir)"
	;;
status)
	domains=$(loaded)
	if [ -z "$domains" ]; then
		echo "$label not loaded"
	fi
	for d in $domains; do
		echo "$d/$label:"
		launchctl print "$d/$label" | grep -E '^	(state|runs|last exit code) =' || true
	done
	;;
*) usage ;;
esac
