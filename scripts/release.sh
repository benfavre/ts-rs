#!/bin/bash
set -euo pipefail

# tsc-rs release script
#
# Usage:
#   ./scripts/release.sh [patch|minor|major]   Bump version, commit, tag, push, monitor CI
#   ./scripts/release.sh --monitor [TAG]        Monitor CI for a tag (default: latest)
#   ./scripts/release.sh --check [TAG]          Verify a release is complete
#   ./scripts/release.sh --status               Show current version and release state
#   ./scripts/release.sh --list-versions        Show all version locations and values
#
# Options:
#   --no-monitor    Skip CI monitoring after push
#
# Requires: gh (GitHub CLI) for monitoring and verification

cd "$(git rev-parse --show-toplevel)"

REPO=$(gh repo view --json nameWithOwner -q '.nameWithOwner' 2>/dev/null || \
       git remote get-url origin | sed 's|.*github\.com[:/]\(.*\)\.git|\1|')

# Workspace version is the single source of truth
WORKSPACE_TOML="Cargo.toml"

# npm packages to bump
NPM_PACKAGES=(
    npm/tsc-rs/package.json
    npm/tsc-rs-linux-x64/package.json
    npm/tsc-rs-linux-arm64/package.json
    npm/tsc-rs-darwin-x64/package.json
    npm/tsc-rs-darwin-arm64/package.json
    npm/tsc-rs-win32-x64/package.json
)

# VSCode extension
VSCODE_PKG="editors/vscode/package.json"

# Expected release artifacts
EXPECTED_ARTIFACTS=(
    "tsc-rs-x86_64-unknown-linux-gnu.tar.gz"
    "tsc-rs-aarch64-unknown-linux-gnu.tar.gz"
    "tsc-rs-x86_64-apple-darwin.tar.gz"
    "tsc-rs-aarch64-apple-darwin.tar.gz"
    "tsc-rs-x86_64-pc-windows-msvc.zip"
    "tsc-rs.vsix"
    "SHA256SUMS.txt"
)

# ─── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
RESET='\033[0m'

info()  { printf "${BLUE}==>${RESET} %s\n" "$1"; }
ok()    { printf "${GREEN}==>${RESET} %s\n" "$1"; }
warn()  { printf "${YELLOW}==>${RESET} %s\n" "$1"; }
err()   { printf "${RED}error:${RESET} %s\n" "$1" >&2; }
die()   { err "$1"; exit 1; }

# ─── Helpers ──────────────────────────────────────────────────────────────────

require_gh() {
    if ! command -v gh &>/dev/null; then
        die "gh (GitHub CLI) is required for this command. Install: https://cli.github.com"
    fi
    if ! gh auth status &>/dev/null 2>&1; then
        die "gh is not authenticated. Run: gh auth login"
    fi
}

current_version() {
    grep '^version' "$WORKSPACE_TOML" | head -1 | sed 's/.*"\(.*\)".*/\1/'
}

latest_tag() {
    git describe --tags --abbrev=0 2>/dev/null || echo ""
}

fmt_duration() {
    local secs=$1
    if (( secs < 60 )); then
        echo "${secs}s"
    elif (( secs < 3600 )); then
        echo "$((secs / 60))m $((secs % 60))s"
    else
        echo "$((secs / 3600))h $((secs % 3600 / 60))m"
    fi
}

# ─── Update all version locations ─────────────────────────────────────────────

bump_all_versions() {
    local old="$1"
    local new="$2"
    local count=0

    # Update workspace version in root Cargo.toml
    sed -i "s/^version = \"$old\"/version = \"$new\"/" "$WORKSPACE_TOML"
    count=$((count + 1))

    # Update npm package.json files
    for pkg in "${NPM_PACKAGES[@]}"; do
        if [[ -f "$pkg" ]]; then
            sed -i "s/\"version\": \"$old\"/\"version\": \"$new\"/" "$pkg"
            count=$((count + 1))
        fi
    done

    # Update optionalDependencies in main npm package
    if [[ -f "npm/tsc-rs/package.json" ]]; then
        sed -i "s/\"@tsc-rs\/linux-x64\": \"$old\"/\"@tsc-rs\/linux-x64\": \"$new\"/" npm/tsc-rs/package.json
        sed -i "s/\"@tsc-rs\/linux-arm64\": \"$old\"/\"@tsc-rs\/linux-arm64\": \"$new\"/" npm/tsc-rs/package.json
        sed -i "s/\"@tsc-rs\/darwin-x64\": \"$old\"/\"@tsc-rs\/darwin-x64\": \"$new\"/" npm/tsc-rs/package.json
        sed -i "s/\"@tsc-rs\/darwin-arm64\": \"$old\"/\"@tsc-rs\/darwin-arm64\": \"$new\"/" npm/tsc-rs/package.json
        sed -i "s/\"@tsc-rs\/win32-x64\": \"$old\"/\"@tsc-rs\/win32-x64\": \"$new\"/" npm/tsc-rs/package.json
    fi

    # Update VSCode extension version
    if [[ -f "$VSCODE_PKG" ]]; then
        sed -i "s/\"version\": \"$old\"/\"version\": \"$new\"/" "$VSCODE_PKG"
        count=$((count + 1))
    fi

    ok "Updated version in $count files: $old → $new"
}

# ─── Command: --list-versions ────────────────────────────────────────────────

cmd_list_versions() {
    echo ""
    printf "${BOLD}tsc-rs Version Locations${RESET}\n"
    echo "───────────────────────────────────────────────────────"

    printf "\n  ${BOLD}Workspace (Cargo.toml):${RESET}\n"
    local ver
    ver=$(current_version)
    printf "    ${CYAN}%-50s${RESET} %s\n" "Cargo.toml [workspace.package] version" "$ver"

    printf "\n  ${BOLD}npm packages:${RESET}\n"
    for pkg in "${NPM_PACKAGES[@]}"; do
        if [[ -f "$pkg" ]]; then
            local pver
            pver=$(grep '"version"' "$pkg" | head -1 | sed 's/.*"\([0-9][^"]*\)".*/\1/')
            printf "    ${CYAN}%-50s${RESET} %s\n" "$pkg" "$pver"
        fi
    done

    printf "\n  ${BOLD}VSCode extension:${RESET}\n"
    if [[ -f "$VSCODE_PKG" ]]; then
        local pver
        pver=$(grep '"version"' "$VSCODE_PKG" | head -1 | sed 's/.*"\([0-9][^"]*\)".*/\1/')
        printf "    ${CYAN}%-50s${RESET} %s\n" "$VSCODE_PKG" "$pver"
    fi

    echo ""
}

# ─── Command: --status ────────────────────────────────────────────────────────

cmd_status() {
    local ver tag
    ver=$(current_version)
    tag=$(latest_tag)

    echo ""
    printf "${BOLD}tsc-rs Release Status${RESET}\n"
    echo "──────────────────────────────"
    printf "  Current version:  ${CYAN}%s${RESET}\n" "$ver"
    printf "  Latest local tag: ${CYAN}%s${RESET}\n" "${tag:-none}"
    printf "  Current branch:   ${CYAN}%s${RESET}\n" "$(git branch --show-current)"
    echo ""

    if [[ -n "$tag" ]] && command -v gh &>/dev/null; then
        local release_status
        release_status=$(gh release view "$tag" --json isDraft,isPrerelease,tagName,publishedAt,assets \
            -q '"  Release: \(.tagName)  Published: \(.publishedAt)  Assets: \(.assets | length)"' 2>/dev/null || echo "")
        if [[ -n "$release_status" ]]; then
            echo "$release_status"
        else
            warn "No GitHub release found for $tag"
        fi

        local run_status
        run_status=$(gh run list --workflow=release.yml --limit=1 \
            --json status,conclusion,headBranch,displayTitle,updatedAt \
            -q '.[0] | "  CI: \(.displayTitle)  Status: \(.status)/\(.conclusion)  (\(.updatedAt))"' 2>/dev/null || echo "")
        if [[ -n "$run_status" ]]; then
            echo "$run_status"
        fi
    fi
    echo ""
}

# ─── Command: --monitor ──────────────────────────────────────────────────────

cmd_monitor() {
    require_gh
    local tag="${1:-$(latest_tag)}"
    [[ -z "$tag" ]] && die "No tag specified and no tags found"

    info "Monitoring CI for $tag..."
    echo ""

    local run_id=""
    local attempts=0
    local max_attempts=30

    while [[ -z "$run_id" || "$run_id" == "null" ]]; do
        run_id=$(gh run list --workflow=release.yml --limit=5 \
            --json databaseId,headBranch,displayTitle,event \
            -q ".[] | select(.displayTitle | contains(\"$tag\")) | .databaseId" 2>/dev/null | head -1 || echo "")

        if [[ -z "$run_id" || "$run_id" == "null" ]]; then
            attempts=$((attempts + 1))
            if (( attempts >= max_attempts )); then
                die "Timed out waiting for CI run to appear for $tag"
            fi
            printf "\r${DIM}  Waiting for CI run to start... (%ds)${RESET}" "$((attempts * 10))"
            sleep 10
        fi
    done
    printf "\r%-60s\r" " "

    ok "Found CI run: $run_id"
    echo ""

    local start_time=$SECONDS
    local prev_status=""

    while true; do
        local run_json
        run_json=$(gh run view "$run_id" --json status,conclusion,jobs 2>/dev/null)

        local status conclusion
        status=$(echo "$run_json" | jq -r '.status')
        conclusion=$(echo "$run_json" | jq -r '.conclusion')
        local elapsed=$(fmt_duration $((SECONDS - start_time)))

        if [[ "$status" != "$prev_status" ]]; then
            printf "\n${BOLD}  CI Status: %-12s  Elapsed: %s${RESET}\n" "$status" "$elapsed"
            echo "  ───────────────────────────────────────"
            prev_status="$status"
        fi

        local jobs
        jobs=$(echo "$run_json" | jq -r '.jobs[] | "\(.name)|\(.status)|\(.conclusion)"' 2>/dev/null || echo "")
        if [[ -n "$jobs" ]]; then
            printf "\r"
            while IFS='|' read -r name jstatus jconclusion; do
                local icon
                case "$jconclusion" in
                    success)   icon="${GREEN}✓${RESET}" ;;
                    failure)   icon="${RED}✗${RESET}" ;;
                    cancelled) icon="${YELLOW}⊘${RESET}" ;;
                    skipped)   icon="${DIM}○${RESET}" ;;
                    *)
                        case "$jstatus" in
                            in_progress) icon="${CYAN}●${RESET}" ;;
                            queued)      icon="${DIM}◌${RESET}" ;;
                            *)           icon="${DIM}?${RESET}" ;;
                        esac
                        ;;
                esac
                printf "  %b %-45s %s/%s\n" "$icon" "$name" "$jstatus" "$jconclusion"
            done <<< "$jobs"
        fi

        if [[ "$status" == "completed" ]]; then
            echo ""
            local total_elapsed=$(fmt_duration $((SECONDS - start_time)))
            if [[ "$conclusion" == "success" ]]; then
                ok "CI completed successfully in $total_elapsed"
                echo ""
                cmd_check "$tag"
                return 0
            else
                err "CI failed with conclusion: $conclusion (after $total_elapsed)"
                echo ""
                echo "  View logs: gh run view $run_id --log-failed"
                echo "  Web:       https://github.com/$REPO/actions/runs/$run_id"
                return 1
            fi
        fi

        sleep 15
    done
}

# ─── Command: --check ─────────────────────────────────────────────────────────

cmd_check() {
    require_gh
    local tag="${1:-$(latest_tag)}"
    [[ -z "$tag" ]] && die "No tag specified and no tags found"

    info "Verifying release $tag..."
    echo ""

    local release_json
    release_json=$(gh release view "$tag" --json tagName,isDraft,assets 2>/dev/null) || \
        die "No GitHub release found for $tag"

    local is_draft
    is_draft=$(echo "$release_json" | jq -r '.isDraft')
    if [[ "$is_draft" == "true" ]]; then
        warn "Release $tag is still a draft"
    fi

    local assets
    assets=$(echo "$release_json" | jq -r '.assets[].name' 2>/dev/null || echo "")

    local all_ok=true
    local found=0
    local missing=0

    printf "  ${BOLD}%-55s %s${RESET}\n" "Expected Artifact" "Status"
    echo "  ─────────────────────────────────────────────────────────────"

    for expected in "${EXPECTED_ARTIFACTS[@]}"; do
        if echo "$assets" | grep -qF "$expected"; then
            printf "  ${GREEN}✓${RESET} %-53s ${GREEN}found${RESET}\n" "$expected"
            found=$((found + 1))
        else
            printf "  ${RED}✗${RESET} %-53s ${RED}missing${RESET}\n" "$expected"
            missing=$((missing + 1))
            all_ok=false
        fi
    done

    local extra
    extra=$(echo "$assets" | while read -r asset; do
        is_expected=false
        for exp in "${EXPECTED_ARTIFACTS[@]}"; do
            if [[ "$asset" == "$exp" ]]; then
                is_expected=true
                break
            fi
        done
        if ! $is_expected && [[ -n "$asset" ]]; then
            echo "$asset"
        fi
    done)

    if [[ -n "$extra" ]]; then
        echo ""
        echo "  Extra artifacts:"
        echo "$extra" | while read -r a; do
            printf "  ${DIM}  %s${RESET}\n" "$a"
        done
    fi

    echo ""
    echo "  ─────────────────────────────────────────────────────────────"
    printf "  Found: %d  Missing: %d  Total expected: %d\n" "$found" "$missing" "${#EXPECTED_ARTIFACTS[@]}"

    echo ""
    if $all_ok; then
        ok "Release $tag is complete and all artifacts are accessible"
        echo ""
        echo "  Release: https://github.com/$REPO/releases/tag/$tag"
    else
        warn "Release $tag has issues (see above)"
        return 1
    fi
}

# ─── Parse arguments ──────────────────────────────────────────────────────────

NO_MONITOR=false
COMMAND=""
CMD_ARG=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --status)
            COMMAND="status"
            shift
            ;;
        --list-versions)
            COMMAND="list-versions"
            shift
            ;;
        --monitor)
            COMMAND="monitor"
            shift
            CMD_ARG="${1:-}"
            [[ -n "$CMD_ARG" ]] && shift
            ;;
        --check)
            COMMAND="check"
            shift
            CMD_ARG="${1:-}"
            [[ -n "$CMD_ARG" ]] && shift
            ;;
        --no-monitor)
            NO_MONITOR=true
            shift
            ;;
        --help|-h)
            sed -n '3,12p' "$0" | sed 's/^# \?//'
            exit 0
            ;;
        patch|minor|major)
            COMMAND="release"
            BUMP_TYPE="$1"
            shift
            ;;
        *)
            die "Unknown argument: $1. Run with --help for usage."
            ;;
    esac
done

[[ -z "$COMMAND" ]] && COMMAND="release"
[[ "$COMMAND" == "release" ]] && BUMP_TYPE="${BUMP_TYPE:-patch}"

# ─── Dispatch ─────────────────────────────────────────────────────────────────

case "$COMMAND" in
    status)        cmd_status; exit 0 ;;
    list-versions) cmd_list_versions; exit 0 ;;
    monitor)       cmd_monitor "$CMD_ARG"; exit $? ;;
    check)         cmd_check "$CMD_ARG"; exit $? ;;
    release)       ;; # continue below
esac

# ─── Command: release (bump, commit, tag, push, monitor) ─────────────────────

CURRENT_VERSION=$(current_version)
if [[ -z "$CURRENT_VERSION" ]]; then
    die "Could not read version from $WORKSPACE_TOML"
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

case "$BUMP_TYPE" in
    patch) PATCH=$((PATCH + 1)) ;;
    minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
    major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
TAG="v${NEW_VERSION}"

echo ""
printf "${BOLD}tsc-rs Release${RESET}\n"
echo "──────────────────────────────"
printf "  Current: ${DIM}%s${RESET}\n" "$CURRENT_VERSION"
printf "  New:     ${CYAN}%s${RESET}  (%s bump)\n" "$NEW_VERSION" "$BUMP_TYPE"
printf "  Tag:     ${CYAN}%s${RESET}\n" "$TAG"
echo ""

# ─── Safety checks ────────────────────────────────────────────────────────────

if ! git diff --quiet -- ':!Cargo.toml' ':!Cargo.lock'; then
    die "You have uncommitted changes. Commit or stash them first.\n$(git diff --stat)"
fi
if ! git diff --cached --quiet; then
    die "You have staged changes. Commit or unstage them first.\n$(git diff --cached --stat)"
fi
ok "Working tree is clean"

if git tag -l "$TAG" | grep -q "$TAG"; then
    die "Local tag $TAG already exists. Delete it with: git tag -d $TAG"
fi

git fetch --tags --quiet 2>/dev/null || true
if git ls-remote --tags origin "refs/tags/$TAG" 2>/dev/null | grep -q "$TAG"; then
    die "Remote tag $TAG already exists on origin. This version has already been released."
fi
ok "Tag $TAG is available"

BRANCH=$(git branch --show-current)
if [[ "$BRANCH" != "main" && "$BRANCH" != "master" ]]; then
    warn "You're on branch '$BRANCH', not 'main'."
    read -rp "   Continue anyway? [y/N] " CONFIRM
    [[ "$CONFIRM" =~ ^[Yy]$ ]] || exit 1
fi

git fetch --quiet
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main 2>/dev/null || git rev-parse origin/master 2>/dev/null || echo "")
if [[ -n "$REMOTE" && "$LOCAL" != "$REMOTE" ]]; then
    BASE=$(git merge-base HEAD "$REMOTE" 2>/dev/null || echo "")
    if [[ "$BASE" != "$LOCAL" ]]; then
        die "Your branch is behind remote. Run 'git pull --rebase' first."
    fi
fi
ok "Branch is up to date with origin"

# ─── Update versions ─────────────────────────────────────────────────────────
echo ""
info "Updating version across workspace and packages..."
bump_all_versions "$CURRENT_VERSION" "$NEW_VERSION"

# ─── Build check ──────────────────────────────────────────────────────────────
info "Running cargo check..."
cargo check --workspace --quiet 2>&1
ok "Build check passed"

# ─── Commit message ──────────────────────────────────────────────────────────
echo ""
info "Recent commits since last tag:"
LAST_TAG=$(latest_tag)
if [[ -n "$LAST_TAG" ]]; then
    git log --oneline "$LAST_TAG"..HEAD | head -10 | sed 's/^/  /'
else
    git log --oneline -10 | sed 's/^/  /'
fi

echo ""
DEFAULT_MSG="$TAG: "
read -rp "  Commit message [$DEFAULT_MSG]: " USER_MSG
COMMIT_MSG="${USER_MSG:-$DEFAULT_MSG}"

if [[ ! "$COMMIT_MSG" =~ ^v[0-9] ]]; then
    COMMIT_MSG="$TAG: $COMMIT_MSG"
fi

# ─── Show what will change ────────────────────────────────────────────────────
echo ""
info "Files to be committed:"
git diff --stat | sed 's/^/  /'

# ─── Final confirmation ──────────────────────────────────────────────────────
echo ""
echo "  ─────────────────────────────────────────────────"
printf "  Version:  ${CYAN}%s${RESET} → ${GREEN}%s${RESET}\n" "$CURRENT_VERSION" "$NEW_VERSION"
printf "  Tag:      ${GREEN}%s${RESET}\n" "$TAG"
printf "  Message:  ${DIM}%s${RESET}\n" "$COMMIT_MSG"
echo "  ─────────────────────────────────────────────────"
echo ""
read -rp "  Push release? [Y/n] " CONFIRM
if [[ "$CONFIRM" =~ ^[Nn]$ ]]; then
    warn "Aborted. Reverting version changes..."
    git checkout -- .
    exit 1
fi

# ─── Commit, tag, push ───────────────────────────────────────────────────────
echo ""
info "Committing and tagging..."

git add Cargo.toml Cargo.lock
for pkg in "${NPM_PACKAGES[@]}"; do
    [[ -f "$pkg" ]] && git add "$pkg"
done
[[ -f "$VSCODE_PKG" ]] && git add "$VSCODE_PKG"

git commit -m "$COMMIT_MSG"
git tag "$TAG"

info "Pushing to origin..."
git push
git push origin "$TAG"

echo ""
ok "Release $TAG pushed successfully"
echo ""
echo "  Release: https://github.com/$REPO/releases/tag/$TAG"
echo "  Actions: https://github.com/$REPO/actions"

# ─── Auto-monitor ────────────────────────────────────────────────────────────
if [[ "$NO_MONITOR" == "false" ]] && command -v gh &>/dev/null && gh auth status &>/dev/null 2>&1; then
    echo ""
    read -rp "  Monitor CI and verify release? [Y/n] " MONITOR_CONFIRM
    if [[ ! "$MONITOR_CONFIRM" =~ ^[Nn]$ ]]; then
        echo ""
        cmd_monitor "$TAG"
    fi
else
    if [[ "$NO_MONITOR" == "false" ]]; then
        echo ""
        warn "Install gh CLI to enable automatic CI monitoring and release verification"
        echo "  https://cli.github.com"
    fi
fi
