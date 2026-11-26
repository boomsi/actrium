#!/usr/bin/env bash
# Build and run Actrium for macOS.
#
# Checks JS dependencies, CocoaPods, and the macOS Rust static libraries,
# builds only when needed, then launches the app.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIGURATION="Debug"
DESTINATION="platform=macOS,arch=$(uname -m)"
FORCE_BUILD=false
NO_LAUNCH=false
KEEP_RUNNING=false
URL_ARG=""
COUNTERAPP_PACKAGE="$REPO_ROOT/packages/actrium/keel-guest-examples/counterapp/dist/actrium-counter-1.0.0.actr"

usage() {
  cat <<'USAGE'
Usage: scripts/actrium/run-macos.sh [options]

Options:
  --build           Force a rebuild before launching.
  --url <target>    Launch with --url. A directory path becomes actro://<absolute path>.
  --counterapp      Launch the CounterApp sample from its built .actr package.
  --no-launch       Build only; print the app path.
  --keep-running    Leave existing Actrium instances running.
  -h, --help        Show this help.

Examples:
  scripts/actrium/run-macos.sh
  scripts/actrium/run-macos.sh --build
  scripts/actrium/run-macos.sh --counterapp
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build)
      FORCE_BUILD=true
      shift
      ;;
    --url)
      if [[ $# -lt 2 ]]; then
        echo "--url requires a value" >&2
        exit 2
      fi
      URL_ARG="$2"
      shift 2
      ;;
    --counterapp)
      if [[ ! -f "$COUNTERAPP_PACKAGE" ]]; then
        echo "CounterApp package not found: $COUNTERAPP_PACKAGE" >&2
        echo "Build it first: (cd packages/actrium/keel-guest-examples/counterapp && bun run build:actr)" >&2
        exit 1
      fi
      URL_ARG="$COUNTERAPP_PACKAGE"
      shift
      ;;
    --no-launch)
      NO_LAUNCH=true
      shift
      ;;
    --keep-running)
      KEEP_RUNNING=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

require() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

require xcodebuild
require node
require pod

if [[ ! -d "$REPO_ROOT/packages/actrium/node_modules" ]]; then
  echo "==> Installing JS dependencies..."
  if command -v corepack >/dev/null 2>&1; then
    (cd "$REPO_ROOT" && corepack pnpm install --frozen-lockfile)
  else
    (cd "$REPO_ROOT" && npm install)
  fi
fi

if [[ ! -d "$REPO_ROOT/macos/Pods" ]]; then
  echo "==> Installing CocoaPods..."
  (cd "$REPO_ROOT/macos" && pod install)
fi

CORE_LIB="$REPO_ROOT/crates/actrium-core/ActriumCore.xcframework/macos-arm64_x86_64/libactrium_core.a"
VAULT_LIB="$REPO_ROOT/crates/actrium-vault/ActriumVault.xcframework/macos-arm64_x86_64/libactrium_vault.a"

if [[ ! -f "$CORE_LIB" || ! -f "$VAULT_LIB" ]]; then
  echo "Missing macOS Rust static libraries:" >&2
  echo "  $CORE_LIB" >&2
  echo "  $VAULT_LIB" >&2
  echo "Run: scripts/actrium/build-actrium-rust-bindings.sh apple" >&2
  echo "Use 'apple', not 'apple-macos', so the iOS slices stay in the XCFrameworks." >&2
  exit 1
fi

resolve_app_path() {
  local build_dir
  build_dir="$(xcodebuild \
    -workspace "$REPO_ROOT/macos/Actrium.xcworkspace" \
    -scheme Actrium \
    -configuration "$CONFIGURATION" \
    -destination "$DESTINATION" \
    -showBuildSettings 2>/dev/null \
    | sed -n 's/^[[:space:]]*BUILT_PRODUCTS_DIR = //p' \
    | head -1)"

  if [[ -z "$build_dir" ]]; then
    echo "Could not resolve BUILT_PRODUCTS_DIR from xcodebuild." >&2
    exit 1
  fi

  printf '%s/Actrium.app\n' "$build_dir"
}

APP_PATH="$(resolve_app_path)"

if [[ "$FORCE_BUILD" == true || ! -d "$APP_PATH" ]]; then
  echo "==> Building Actrium ($CONFIGURATION)..."
  (cd "$REPO_ROOT" && npm run verify:macos)
  APP_PATH="$(resolve_app_path)"
fi

if [[ ! -d "$APP_PATH" ]]; then
  echo "Build finished but app was not found at: $APP_PATH" >&2
  exit 1
fi

if [[ "$NO_LAUNCH" == true ]]; then
  echo "$APP_PATH"
  exit 0
fi

if [[ "$KEEP_RUNNING" != true ]]; then
  if pgrep -f -- "$APP_PATH/Contents/MacOS/Actrium" >/dev/null 2>&1; then
    echo "==> Closing existing Actrium instances..."
    pkill -f -- "$APP_PATH/Contents/MacOS/Actrium" 2>/dev/null || true
    for _ in $(seq 1 25); do
      pgrep -f -- "$APP_PATH/Contents/MacOS/Actrium" >/dev/null 2>&1 || break
      sleep 0.2
    done
    if pgrep -f -- "$APP_PATH/Contents/MacOS/Actrium" >/dev/null 2>&1; then
      echo "Existing Actrium did not exit; forcing..." >&2
      pkill -9 -f -- "$APP_PATH/Contents/MacOS/Actrium" 2>/dev/null || true
    fi
  fi
fi

if [[ -n "$URL_ARG" ]]; then
  if [[ "$URL_ARG" != actro://* ]]; then
    if [[ -d "$URL_ARG" ]]; then
      URL_ARG="actro://$(cd "$URL_ARG" && pwd)"
    elif [[ -f "$URL_ARG" ]]; then
      local_path="$URL_ARG"
      if [[ "$local_path" == *.actr ]]; then
        # Keep packages out of TCC-protected folders such as Desktop.
        staged_dir="$HOME/Library/Caches/Actro/packages"
        mkdir -p "$staged_dir"
        staged_pkg="$staged_dir/$(basename "$local_path")"
        if [[ "$local_path" != "$staged_pkg" ]]; then
          cp -f "$local_path" "$staged_pkg"
        fi
        echo "==> Staged package: $staged_pkg"
        local_path="$staged_pkg"
      fi
      URL_ARG="actro://$(cd "$(dirname "$local_path")" && pwd)/$(basename "$local_path")"
    else
      echo "URL target does not exist: $URL_ARG" >&2
      exit 1
    fi
  fi
  echo "==> Launching with $URL_ARG"
  open -a "$APP_PATH" --args --url "$URL_ARG"
else
  echo "==> Launching $APP_PATH"
  open -a "$APP_PATH"
fi
