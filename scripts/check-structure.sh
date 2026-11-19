#!/usr/bin/env bash
# Guards two structure rules of CLAUDE.md:
#   1. Backend crates do not depend on ncrs-ui-kit.
#   2. No Rust file over 1000 lines.
# The allowances below are the current exceptions. They may only shrink.
set -euo pipefail
cd "$(dirname "$0")/.."

backend_crates="ncrs-fs ncrs-index ncrs-index-types ncrs-intake ncrs-pipeline ncrs-ai ncrs-filing ncrs-config ncrs-jobs ncrs-ocr-vision ncrs-priority ncrs-sandbox ncrs-console"
kit_allowance="ncrs-filing"

oversized_allowance="
src/ui_tests/mod.rs
src/e2e_tests/mod.rs
crates/ncrs-index/src/index/mod.rs
crates/ncrs-index/src/documents/mod.rs
crates/ncrs-filing/src/chat_work/tests/mod.rs
crates/ncrs-pipeline/src/runner/mod.rs
crates/ncrs-intake/src/filing_plan/tests/mod.rs
crates/ncrs-index/src/model/mod.rs
crates/ncrs-hermes-chat/tests/rule_flow.rs
"

status=0

for crate in $backend_crates; do
  case " $kit_allowance " in *" $crate "*) continue ;; esac
  # Only the [dependencies] and [build-dependencies] tables, not dev-dependencies.
  if awk '/^\[/ { in_deps = ($0 == "[dependencies]" || $0 == "[build-dependencies]") }
          in_deps && /^ncrs-ui-kit[ =]/ { found = 1 }
          END { exit !found }' "crates/$crate/Cargo.toml"; then
    echo "::error file=crates/$crate/Cargo.toml::backend crate $crate depends on ncrs-ui-kit"
    status=1
  fi
done

while IFS= read -r file; do
  lines=$(wc -l < "$file")
  if [ "$lines" -gt 1000 ] && ! printf '%s\n' "$oversized_allowance" | grep -qxF "$file"; then
    echo "::error file=$file::$file has $lines lines; split it (limit about 1000)"
    status=1
  fi
done < <(git ls-files 'src/*.rs' 'crates/*.rs')

exit $status
