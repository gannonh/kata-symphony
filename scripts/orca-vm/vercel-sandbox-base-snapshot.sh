#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=vercel-sandbox-common.sh
source "$SCRIPT_DIR/vercel-sandbox-common.sh"

resolve_provider_values

base_name="$(env_or_state ORCA_VM_BASE_NAME baseName kata-symphony-orca-base)"
runtime="$(env_or_state ORCA_VM_RUNTIME runtime node24)"
timeout="$(env_or_state ORCA_VM_TIMEOUT timeout 60m)"
vcpus="$(env_or_state ORCA_VM_VCPUS vcpus 4)"
port="$(env_or_state ORCA_VM_PORT port 7331)"
snapshot_expiration="$(env_or_state ORCA_VM_SNAPSHOT_EXPIRATION snapshotExpiration 30d)"
keep_last_snapshots="$(env_or_state ORCA_VM_KEEP_LAST_SNAPSHOTS keepLastSnapshots 2)"
repo_url="$(env_or_state ORCA_REPO_URL repoUrl https://github.com/gannonh/kata-symphony.git)"
repo_ref="$(env_or_state ORCA_REPO_REF repoRef main)"
project_root="$(env_or_state ORCA_PROJECT_ROOT projectRoot /vercel/sandbox/kata-symphony)"
ora_version="$(env_or_state ORCA_VERSION orcaVersion v1.4.168)"
pnpm_version="$(env_or_state PNPM_VERSION pnpmVersion 10.6.2)"
codex_version="$(env_or_state CODEX_VERSION codexVersion 0.146.0)"

[ -n "$scope" ] || fail "VERCEL_TEAM_ID or state.scope is required"
[ -n "$project" ] || fail "VERCEL_PROJECT_ID or state.project is required"
resolve_git_token

name="$(sanitize_name "${base_name}-$(date +%s)")"
created=0
cleanup() {
  local status=$?
  trap - EXIT
  if ((status != 0 && created == 1)); then
    vercel sandbox remove "$name" "${vercel_args[@]}" >/dev/null 2>&1 || true
  fi
  exit "$status"
}
trap cleanup EXIT

create_output="$(vercel sandbox create \
  --name "$name" \
  --runtime "$runtime" \
  --timeout "$timeout" \
  --vcpus "$vcpus" \
  --publish-port "$port" \
  --snapshot-expiration "$snapshot_expiration" \
  --keep-last-snapshots "$keep_last_snapshots" \
  "${vercel_args[@]}" 2>&1)" || {
  status=$?
  printf '%s\n' "$create_output" >&2
  exit "$status"
}
printf '%s\n' "$create_output" >&2
created=1

IFS= read -r -d '' root_setup <<'REMOTE' || true
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

if command -v dnf >/dev/null 2>&1; then
  dnf install -y file jq util-linux xorg-x11-server-Xvfb xorg-x11-xauth zlib-devel ca-certificates git gcc gcc-c++ make pkgconf-pkg-config openssl-devel nss nspr gtk3 libXcomposite libXdamage libXrandr libXtst libXi libXScrnSaver libxkbcommon libxkbcommon-x11 alsa-lib at-spi2-atk pango libnotify cups-libs dbus-libs mesa-libgbm
elif command -v apt-get >/dev/null 2>&1; then
  apt-get update
  apt-get install -y curl file jq xvfb zlib1g-dev ca-certificates git build-essential pkg-config libssl-dev
else
  echo 'The Vercel runtime has no supported dnf or apt-get package manager.' >&2
  exit 1
fi
command -v npm >/dev/null 2>&1 || { echo 'Node/npm is missing from the Vercel runtime.' >&2; exit 1; }

case "$(uname -m)" in
  x86_64|amd64) ORCA_ASSET='orca-linux.AppImage'; ORCA_FILE_MACHINE='x86-64' ;;
  aarch64|arm64) ORCA_ASSET='orca-linux-arm64.AppImage'; ORCA_FILE_MACHINE='ARM aarch64' ;;
  *) echo "Unsupported sandbox architecture: $(uname -m)" >&2; exit 1 ;;
esac

install -d -m 0755 /opt/orca
curl -fL --retry 3 \
  "https://github.com/stablyai/orca/releases/download/${ORCA_VERSION}/${ORCA_ASSET}" \
  -o /opt/orca/orca-linux.AppImage
chmod 0755 /opt/orca/orca-linux.AppImage
file_info="$(file /opt/orca/orca-linux.AppImage)"
printf '%s\n' "$file_info" >&2
grep -F 'ELF' <<<"$file_info"
grep -F "$ORCA_FILE_MACHINE" <<<"$file_info"

cd /opt/orca
rm -rf squashfs-root
./orca-linux.AppImage --appimage-extract >/tmp/orca-appimage-extract.log 2>&1
test -x squashfs-root/AppRun
cat >/usr/local/bin/orca <<'WRAPPER'
#!/usr/bin/env bash
set -euo pipefail

app_dir=/opt/orca/squashfs-root
electron="$app_dir/orca-ide"
cli="$app_dir/resources/app.asar.unpacked/out/cli/index.js"

export LD_LIBRARY_PATH="$app_dir/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export ORCA_NODE_OPTIONS="${NODE_OPTIONS-}"
export ORCA_NODE_REPL_EXTERNAL_MODULE="${NODE_REPL_EXTERNAL_MODULE-}"
unset NODE_OPTIONS NODE_REPL_EXTERNAL_MODULE
export ELECTRON_DISABLE_SANDBOX=1
export ELECTRON_RUN_AS_NODE=1
if [ -z "${DISPLAY:-}" ]; then
  display_number="${ORCA_XVFB_DISPLAY_NUMBER:-99}"
  display=":$display_number"
  socket="/tmp/.X11-unix/X$display_number"
  pid_file="/tmp/orca-xvfb.pid"
  xvfb_running() {
    local xvfb_pid=""
    [ -f "$pid_file" ] && read -r xvfb_pid <"$pid_file"
    [[ "$xvfb_pid" =~ ^[0-9]+$ ]] &&
      [ "$(cat "/proc/$xvfb_pid/comm" 2>/dev/null || true)" = Xvfb ] &&
      [ -S "$socket" ]
  }
  (
    flock -x 9
    if ! xvfb_running; then
      rm -f "$socket" "/tmp/.X${display_number}-lock" "$pid_file"
      nohup Xvfb "$display" -screen 0 1280x720x24 -nolisten tcp -noreset \
        >/tmp/orca-xvfb.log 2>&1 </dev/null &
      printf '%s' "$!" >"$pid_file"
    fi
  ) 9>/tmp/orca-xvfb.lock
  for _ in $(seq 1 100); do
    xvfb_running && break
    sleep 0.05
  done
  xvfb_running || { cat /tmp/orca-xvfb.log >&2; exit 1; }
  export DISPLAY="$display"
fi
exec "$electron" "$cli" "$@"
WRAPPER
chmod 0755 /usr/local/bin/orca
printf '%s\n' "$ORCA_VERSION" >/opt/orca/VERSION

npm install --global "pnpm@${PNPM_VERSION}" "@openai/codex@${CODEX_VERSION}"
printf '%s\n' '__ORCA_ROOT_SETUP_OK__'
REMOTE
root_setup_output="$(vercel sandbox exec "$name" "${vercel_args[@]}" --sudo --timeout 20m \
  --env "ORCA_VERSION=$ora_version" \
  --env "PNPM_VERSION=$pnpm_version" \
  --env "CODEX_VERSION=$codex_version" \
  -- bash -lc "$root_setup" 2>&1)" || {
  status=$?
  printf '%s\n' "$root_setup_output" >&2
  exit "$status"
}
printf '%s\n' "$root_setup_output" >&2
grep -Fx '__ORCA_ROOT_SETUP_OK__' <<<"$root_setup_output" >/dev/null || fail "base sandbox prerequisite setup did not reach its success marker"

IFS= read -r -d '' remote_build <<'REMOTE' || true
set -euo pipefail
export PATH="/usr/local/bin:$HOME/.local/bin:$HOME/.cargo/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  source "$HOME/.cargo/env"
fi
rustup default stable >/dev/null

rm -rf "$ORCA_PROJECT_ROOT"
mkdir -p "$(dirname "$ORCA_PROJECT_ROOT")"
if [ -n "${GH_TOKEN:-}" ]; then
  printf '%s\n' '#!/usr/bin/env bash' 'case "$1" in *Username*) printf "%s\\n" x-access-token ;; *Password*) printf "%s\\n" "$GH_TOKEN" ;; esac' >/tmp/orca-askpass.sh
  chmod 0700 /tmp/orca-askpass.sh
  trap 'rm -f /tmp/orca-askpass.sh' EXIT
  export GIT_ASKPASS=/tmp/orca-askpass.sh GIT_TERMINAL_PROMPT=0
fi
git clone "$ORCA_REPO_URL" "$ORCA_PROJECT_ROOT"
cd "$ORCA_PROJECT_ROOT"
git checkout -B "$ORCA_REPO_REF" "origin/$ORCA_REPO_REF"
rm -f /tmp/orca-askpass.sh

pnpm install --frozen-lockfile
pnpm run build
printf '%s' "$(git rev-parse HEAD)" >.orca-built

test -f apps/cli/dist/loader.js
test -x apps/symphony/target/release/symphony
command -v codex >/dev/null 2>&1
command -v orca >/dev/null 2>&1
orca serve --help >/dev/null
codex --version >/dev/null
printf '%s\n' '__ORCA_BASE_BUILD_OK__'
REMOTE
build_output="$(vercel sandbox exec "$name" "${vercel_args[@]}" --timeout 50m \
  --env "GH_TOKEN=$gh_token" \
  --env "ORCA_PROJECT_ROOT=$project_root" \
  --env "ORCA_REPO_URL=$repo_url" \
  --env "ORCA_REPO_REF=$repo_ref" \
  -- bash -lc "$remote_build" 2>&1)" || {
  status=$?
  printf '%s\n' "$build_output" >&2
  exit "$status"
}
printf '%s\n' "$build_output" >&2
grep -Fx '__ORCA_BASE_BUILD_OK__' <<<"$build_output" >/dev/null || fail "base sandbox build did not reach its success marker"

snapshot_output="$(vercel sandbox snapshot "$name" --stop --expiration "$snapshot_expiration" "${vercel_args[@]}" 2>&1)" || {
  status=$?
  printf '%s\n' "$snapshot_output" >&2
  exit "$status"
}
printf '%s\n' "$snapshot_output" >&2
snapshot_id="$(parse_snapshot_id "$snapshot_output")"
[ -n "$snapshot_id" ] || fail "Vercel snapshot output did not contain a snapshot id"

vercel sandbox remove "$name" "${vercel_args[@]}" >&2
created=0

patch="$(node -e '
  const [baseName, snapshotId, scope, project, port, repoUrl, repoRef, projectRoot, runtime, timeout, vcpus, snapshotExpiration, keepLastSnapshots, orcaVersion, codexVersion, pnpmVersion] = process.argv.slice(1);
  process.stdout.write(JSON.stringify({
    baseName,
    baseSandboxName: "",
    snapshotId,
    authSourceSnapshotId: "",
    authSandboxName: "",
    scope,
    project,
    port: Number(port),
    repoUrl,
    repoRef,
    projectRoot,
    runtime,
    timeout,
    vcpus: Number(vcpus),
    snapshotExpiration,
    keepLastSnapshots: Number(keepLastSnapshots),
    orcaVersion,
    codexVersion,
    pnpmVersion
  }));
' "$base_name" "$snapshot_id" "$scope" "$project" "$port" "$repo_url" "$repo_ref" "$project_root" "$runtime" "$timeout" "$vcpus" "$snapshot_expiration" "$keep_last_snapshots" "$ora_version" "$codex_version" "$pnpm_version")"
state_merge "$patch"
trap - EXIT
print_state
