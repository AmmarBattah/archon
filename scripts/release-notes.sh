#!/bin/sh
# Prints the body of a release: the section of CHANGELOG.md for one version,
# then how to install. Usage: release-notes.sh VERSION [CHANGELOG]
set -eu

ARCHON_REPOSITORY="mengsig/archon"

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

# Prints what is under `## VERSION`, up to the next `## ` heading outside a
# code block, without the blank lines before it.
section_of() {
    awk -v heading="## $1" '
        /^```/ { fenced = !fenced }
        found && !fenced && /^## / { exit }
        found && (started || NF) { started = 1; print }
        $0 == heading && !fenced { found = 1 }
    ' "$2"
}

main() {
    [ "$#" -eq 1 ] || [ "$#" -eq 2 ] || die "usage: release-notes.sh VERSION [CHANGELOG]"
    version=${1#v}
    changelog=${2:-CHANGELOG.md}
    printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' ||
        die "the version must look like 1.2.3, not '$1'"
    [ -f "$changelog" ] || die "$changelog does not exist"

    section=$(section_of "$version" "$changelog")
    [ -n "$section" ] ||
        die "$changelog has no '## $version' section with text in it; write it before releasing $version"

    installer="https://raw.githubusercontent.com/$ARCHON_REPOSITORY/main/install.sh"
    cat <<NOTES
$section

### Install

\`\`\`sh
curl -fsSL $installer | sh
\`\`\`

This installs the latest release. To install this one, put
\`ARCHON_VERSION=v$version\` before \`sh\`. Other ways to install are in
[docs/install.md](https://github.com/$ARCHON_REPOSITORY/blob/v$version/docs/install.md).
NOTES
}

main "$@"
