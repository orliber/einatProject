#!/usr/bin/env bash
# A new version for Einat's computer (D-033, D-091). From the repository root.
#
#   scripts/release.sh bump <version>     a PR that sets the version, with its "מה חדש" file and
#                                         the vault it keeps for later versions (main is protected)
#   scripts/release.sh <version>          after that PR is merged and CI on main is green:
#                                         starts the release workflow on GitHub and follows it
#   scripts/release.sh <version> --sign-here
#                                         the same release, signed on this computer instead of
#                                         with the repository secret (the key stays here)
#   scripts/release.sh setup              once: signing key + the public releases repository
#   scripts/release.sh secrets            once: puts the signing key and the releases token in
#                                         the repository's secrets, so GitHub can release alone
#
# Nothing is built at release time. Every merge to main already built the Windows installer as
# it is published and ran every release check on it (ci.yml → windows-release). The release
# takes that installer, checks it is the same commit and version, signs the notice (version,
# notes, SHA-256) and checks it with the program's own code, and publishes. A few minutes.
set -euo pipefail

SOURCE_REPO="orliber/einatProject"
RELEASES_REPO="orliber/einat-vault-releases"
KEY="${DV_UPDATE_KEY:-$HOME/.diagnostic-vault/update-signing.pk8}"
OUT="release-out"

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
die() { printf '\n\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null || die "חסר $1 (brew install $2)"; }
need gh gh
gh auth status >/dev/null 2>&1 || die "קודם: gh auth login"

if [[ "${1:-}" == "setup" ]]; then
  need cargo rust
  say "1/2 מפתח חתימה"
  if [[ -f "$KEY" ]]; then
    echo "כבר קיים: $KEY (לא נוצר חדש)"
  else
    cargo xtask update-keygen
    echo "חשוב: לגבות את $KEY למקום בטוח ולא מחובר (למשל דיסק און קי בכספת)."
  fi
  say "2/2 ריפו ציבורי להתקנות בלבד"
  if gh repo view "$RELEASES_REPO" >/dev/null 2>&1; then
    echo "כבר קיים: $RELEASES_REPO"
  else
    gh repo create "$RELEASES_REPO" --public \
      --description "Installers for Diagnostic Vault. No code, no data: signed installers only."
    tmp="$(mktemp -d)"
    cat > "$tmp/README.md" <<'EOF'
# Diagnostic Vault – installers

Signed Windows installers only. No source code and no data of any kind live here.
Every release carries `latest.json` and `latest.json.sig` (Ed25519); the program installs a
new version only if the signature matches the key built into it and the installer's SHA-256
matches the signed notice.
EOF
    gh api -X PUT "repos/$RELEASES_REPO/contents/README.md" \
      -f message="README" -f content="$(base64 < "$tmp/README.md" | tr -d '\n')" >/dev/null
    rm -r "$tmp"
  fi
  say "סיום. עכשיו מכניסים את crates/dv-egress/update_key.pub (המפתח הציבורי בלבד) ל-main ב-PR רגיל."
  exit 0
fi

if [[ "${1:-}" == "secrets" ]]; then
  [[ -f "$KEY" ]] || die "אין מפתח חתימה ב-$KEY. קודם: scripts/release.sh setup (או update-key-restore מהגיבוי)"
  say "1/2 מפתח החתימה → הסוד DV_UPDATE_SIGNING_KEY"
  # gh encrypts the value for the repository before it leaves this computer; it is never
  # printed, and Actions shows it to no one (D-091).
  base64 < "$KEY" | tr -d '\n' | gh secret set DV_UPDATE_SIGNING_KEY -R "$SOURCE_REPO"
  say "2/2 הרשאה לפרסם בריפו ההתקנות → הסוד RELEASES_REPO_TOKEN"
  echo "ב-GitHub: Settings ← Developer settings ← Fine-grained tokens ← Generate new token."
  echo "Repository access: רק $RELEASES_REPO. Permissions: Contents → Read and write. תוקף: שנה."
  echo "מדביקים כאן את הטוקן (לא יוצג על המסך):"
  gh secret set RELEASES_REPO_TOKEN -R "$SOURCE_REPO"
  say "סיום. מעכשיו גרסה יוצאת מ-GitHub: Actions ← Release ← Run workflow, או scripts/release.sh <version>."
  exit 0
fi

set_version() {
  perl -0pi -e "s/(\[workspace\.package\]\nversion = \")[^\"]+/\${1}$1/" Cargo.toml
  for f in apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json; do
    perl -pi -e "s/^(  \"version\": \")[^\"]+/\${1}$1/ if \$. < 6" "$f"
  done
  cargo check -q -p dv-core
}

if [[ "${1:-}" == "bump" ]]; then
  need cargo rust
  V="${2:-}"
  [[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "שימוש: scripts/release.sh bump 0.2.0"
  [[ -z "$(git status --porcelain)" ]] || die "יש שינויים שלא נשמרו ב-git."
  git fetch -q origin main
  git checkout -q -b "release/v$V" origin/main
  set_version "$V"
  # F7: this version keeps a vault, so every later version proves it still opens it.
  [[ -d "tests/fixtures/vaults/$V" ]] || DV_WRITE_VAULT_FIXTURE="$V" cargo test -q -p dv-core upgrade::write -- --ignored
  notes="release-notes/$V.txt"
  if [[ ! -s "$notes" ]]; then
    mkdir -p release-notes
    printf '%s\n' "שורה לכל שינוי, בעברית פשוטה לעינת" > "$notes"
    "${EDITOR:-nano}" "$notes"
  fi
  grep -q 'שורה לכל שינוי' "$notes" && die "קודם לכתוב את $notes (שורה לכל שינוי)"
  scripts/release-ready.sh >/dev/null
  git add Cargo.toml Cargo.lock apps/desktop/src-tauri/tauri.conf.json apps/desktop/package.json \
    "tests/fixtures/vaults/$V" "$notes"
  git commit -q -m "Version $V"
  git push -q -u origin "release/v$V"
  gh pr create --repo "$SOURCE_REPO" --base main --head "release/v$V" \
    --title "Version $V" --body "מספר הגרסה $V, מה חדש ($notes) והכספת שהגרסה שומרת. אחרי המיזוג ו-CI ירוק: scripts/release.sh $V"
  say "נפתח PR לגרסה $V. אחרי שהוא ממוזג ו-CI על main ירוק: scripts/release.sh $V"
  exit 0
fi

VERSION="${1:-}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "שימוש: scripts/release.sh 0.2.0 [--sign-here]"
main_version="$(gh api "repos/$SOURCE_REPO/contents/Cargo.toml?ref=main" -H 'Accept: application/vnd.github.raw' \
  | grep -m1 '^version = ' | cut -d'"' -f2)"
[[ "$main_version" == "$VERSION" ]] \
  || die "הגרסה ב-main היא $main_version ולא $VERSION. קודם: scripts/release.sh bump $VERSION (ולמזג את ה-PR)."

if [[ "${2:-}" != "--sign-here" ]]; then
  say "מתחילים את השחרור ב-GitHub"
  gh workflow run release.yml -R "$SOURCE_REPO" --ref main
  sleep 8
  run_id="$(gh run list -R "$SOURCE_REPO" --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
  [[ -n "$run_id" ]] || die "לא נמצאה ריצת שחרור"
  gh run watch "$run_id" -R "$SOURCE_REPO" --exit-status \
    || die "השחרור נעצר. מה קרה: gh run view $run_id -R $SOURCE_REPO --log-failed"
  say "הגרסה $VERSION פורסמה. אצל עינת היא תופיע כ'יש גרסה חדשה' בבדיקה היומית הבאה (או מיד, מ'הגדרות ← בדיקת עדכונים')."
  exit 0
fi

# --sign-here: the installer CI built and checked, signed with the key on this computer.
need cargo rust
[[ -f "$KEY" ]] || die "אין מפתח חתימה ב-$KEY"
git fetch -q origin main
sha="$(git rev-parse origin/main)"
say "1/4 המתקין ש-CI בנה ובדק ל-$sha"
rm -rf "$OUT" && mkdir -p "$OUT"
# One green CI run (not a pull request's): both platforms' files come from it.
got="$(gh run list -R "$SOURCE_REPO" --workflow ci.yml --commit "$sha" --limit 20 --json databaseId,event,conclusion \
    -q '[.[] | select(.event != "pull_request" and .conclusion == "success")] | .[0].databaseId // empty')" \
  || die "לא הצלחתי לקרוא את ריצות ה-CI של $sha. לנסות שוב."
[[ -n "$got" ]] || die "אין עדיין ריצת CI ירוקה ל-$sha: CI על main בונה את המתקינים אחרי כל מיזוג. לחכות שיהיה ירוק."
gh run download "$got" -R "$SOURCE_REPO" -n release-windows -D "$OUT" \
  || die "אין מתקין Windows בריצה $got (ci.yml → windows-release)."
grep -qx "commit=$sha" "$OUT/build.txt" && grep -qx "version=$VERSION" "$OUT/build.txt" \
  || die "המתקין לא של $sha בגרסה $VERSION: $(cat "$OUT/build.txt")"
# Written on Windows (CRLF): without the CR, shasum looks for the right name.
(cd "$OUT" && for f in *.sha256; do tr -d '\r' < "$f" | shasum -a 256 -c -; done)
# The Mac disk image of the same run (D-101).
gh run download "$got" -R "$SOURCE_REPO" -n release-macos -D "$OUT/mac" \
  || die "אין דמות דיסק של Mac בריצה $got (ci.yml → macos-release)."
grep -qx "commit=$sha" "$OUT/mac/build.txt" && grep -qx "version=$VERSION" "$OUT/mac/build.txt" \
  || die "דמות הדיסק לא של $sha בגרסה $VERSION: $(cat "$OUT/mac/build.txt")"
(cd "$OUT/mac" && shasum -a 256 -c DiagnosticVault.dmg.sha256)
mv "$OUT/mac/DiagnosticVault.dmg" "$OUT/mac/DiagnosticVault.dmg.sha256" "$OUT/" && rm -r "$OUT/mac"

say "2/4 חתימה ובדיקה עצמית"
git show "origin/main:release-notes/$VERSION.txt" > "$OUT/notes.txt"
cargo xtask update-notice "$VERSION" "$OUT/DiagnosticVault-Setup.exe" "$OUT/notes.txt" "$OUT" "$OUT/DiagnosticVault.dmg"

say "3/4 פרסום ב-$SOURCE_REPO"
files=("$OUT/DiagnosticVault-Setup.exe" "$OUT/DiagnosticVault-Setup.exe.sha256"
       "$OUT/DiagnosticVault.dmg" "$OUT/DiagnosticVault.dmg.sha256"
       "$OUT/DiagnosticVault-sbom.cdx.json" "$OUT/DiagnosticVault-sbom.cdx.json.sha256")
if gh release view "v$VERSION" -R "$SOURCE_REPO" >/dev/null 2>&1; then
  gh release upload "v$VERSION" "${files[@]}" -R "$SOURCE_REPO" --clobber
else
  # A tag left by a release that never published (a failed build) moves to this commit.
  git fetch -q --force origin "refs/tags/v$VERSION:refs/tags/v$VERSION" 2>/dev/null || true
  at="$(git rev-parse -q --verify "refs/tags/v$VERSION^{commit}" || true)"
  if [[ -n "$at" && "$at" != "$sha" ]]; then
    gh api -X DELETE "repos/$SOURCE_REPO/git/refs/tags/v$VERSION" >/dev/null
  fi
  gh release create "v$VERSION" "${files[@]}" -R "$SOURCE_REPO" --target "$sha" \
    --title "כספת האבחון $VERSION" --notes-file "$OUT/notes.txt"
fi

say "4/4 פרסום ב-$RELEASES_REPO"
gh release create "v$VERSION" --repo "$RELEASES_REPO" --latest \
  --title "כספת האבחון $VERSION" --notes-file "$OUT/notes.txt" \
  "$OUT/DiagnosticVault-Setup.exe" "$OUT/DiagnosticVault.dmg" "$OUT/latest.json" "$OUT/latest.json.sig"

say "הגרסה $VERSION פורסמה. אצל עינת היא תופיע כ'יש גרסה חדשה' בבדיקה היומית הבאה (או מיד, מ'הגדרות ← בדיקת עדכונים')."
