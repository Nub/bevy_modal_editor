#!/usr/bin/env bash
# M0 Spike 4 — cell-merge (spikes/README.md)
#
# Proves (or refutes): a level stored as a directory of cell files — stably-ordered
# text, entity blocks sorted by UUID, blank-line separated — survives real git merges:
#   S1 different cells edited on two branches            -> clean merge
#   S2 same cell, different entities edited              -> clean merge
#   S3 same cell, both branches add entities             -> clean merge (sorted insert)
#   S4 same entity edited on both branches               -> conflict, confined+readable
#   S5 delete on one branch, edit on the other           -> conflict, survivable
#
# Throwaway: findings go to FINDINGS.md.
set -euo pipefail

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
git init -q repo && cd repo
git config user.email spike@example.com && git config user.name spike

mkdir -p level/cells

# Deterministic fake UUIDs: sortable, readable.
uid() { printf "%08d-0000-0000-0000-%012d" "$1" "$1"; }

# One entity block. Stable field order, one field per line, blank line after.
block() { # $1=uuid $2=x $3=y $4=z $5=hp
  cat <<EOF
entity "$1" (
    Transform: (translation: ($2, $3, $4)),
    Health: (current: $5, max: 100.0),
)

EOF
}

# Generate cell files: 3 cells x 20 entities, sorted by uuid.
gen_cell() { # $1=cell_name $2=base_index
  local f="level/cells/$1.ron"
  {
    echo "// cell $1 — entities sorted by uuid, one block each"
    for i in $(seq 0 19); do
      block "$(uid $(($2 + i)))" "$i.0" "0.0" "0.0" "100.0"
    done
  } > "$f"
}
gen_cell c_00_00 1000
gen_cell c_00_01 2000
gen_cell c_01_00 3000
cat > level/level.ron <<'EOF'
(format_version: 1, cells: ["c_00_00", "c_00_01", "c_01_00"])
EOF
git add -A && git commit -qm base

edit_entity() { # $1=file $2=uuid $3=new_x
  # Replace the translation line inside the block for $2 (block = uuid line + 3 lines).
  awk -v id="$2" -v x="$3" '
    index($0, id) { inblock=1 }
    inblock && /translation/ { sub(/\(translation: \([^)]*\)\)/, "(translation: (" x ", 9.9, 9.9))"); inblock=0 }
    { print }
  ' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
}

insert_entity() { # $1=file $2=uuid  — insert block at sorted position
  python3 - "$1" "$2" <<'PY'
import sys
path, uid = sys.argv[1], sys.argv[2]
newblock = f'entity "{uid}" (\n    Transform: (translation: (5.0, 5.0, 5.0)),\n    Health: (current: 100.0, max: 100.0),\n)\n\n'
lines = open(path).read().split("\n\n")
# lines[0] starts with comment+first block; find insertion point by uuid compare
blocks = [b for b in lines if b.strip()]
header = ""
if blocks and blocks[0].startswith("//"):
    first = blocks[0].split("\n", 1)
    header = first[0] + "\n"
    blocks[0] = first[1]
def key(b):
    import re
    m = re.search(r'entity "([^"]+)"', b)
    return m.group(1) if m else ""
blocks.append(newblock.strip())
blocks.sort(key=key)
open(path, "w").write(header + "\n\n".join(blocks) + "\n\n")
PY
}

delete_entity() { # $1=file $2=uuid
  awk -v id="$2" '
    index($0, id) { skip=4; next }
    skip > 0 { skip--; next }
    { print }
  ' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
}

scenario() { # $1=name $2=branchA_cmds $3=branchB_cmds $4=expect (clean|conflict)
  local name=$1 expect=$4
  git checkout -qb "${name}_a" main 2>/dev/null || git checkout -qb "${name}_a" master
  eval "$2"; git commit -qam "${name} A"
  git checkout -qb "${name}_b" "$(git merge-base HEAD "${name}_a")" >/dev/null 2>&1 || git checkout -qb "${name}_b" HEAD~1
  eval "$3"; git commit -qam "${name} B"
  if git merge -q --no-edit "${name}_a" >/dev/null 2>&1; then result=clean; else result=conflict; fi
  if [ "$result" = "$expect" ]; then status=PASS; else status=FAIL; fi
  printf "%-52s expect %-8s got %-8s %s\n" "$name" "$expect" "$result" "$status"
  if [ "$result" = conflict ]; then
    conflict_lines=$(git diff --name-only --diff-filter=U | xargs grep -c '^<<<<<<<' 2>/dev/null | awk -F: '{s+=$2} END {print s+0}')
    files=$(git diff --name-only --diff-filter=U | tr '\n' ' ')
    printf "    conflict confined to: %s(%s marker(s))\n" "$files" "$conflict_lines"
    git merge --abort 2>/dev/null || true
  fi
  git checkout -q "$(git rev-list --max-parents=0 HEAD | head -1)" 2>/dev/null
  git checkout -qB work_base "$BASE"
}

BASE=$(git rev-parse HEAD)
git branch -m main 2>/dev/null || true

run() { # name  A-cmds  B-cmds  expect
  git checkout -qB A "$BASE"; eval "$2"; git commit -qam "$1 A"
  git checkout -qB B "$BASE"; eval "$3"; git commit -qam "$1 B"
  if git merge -q --no-edit A >/dev/null 2>&1; then result=clean; else result=conflict; fi
  [ "$result" = "$4" ] && status=PASS || status=FAIL
  printf "%-52s expect %-8s got %-8s %s\n" "$1" "$4" "$result" "$status"
  if [ "$result" = conflict ]; then
    files=$(git diff --name-only --diff-filter=U | tr '\n' ' ')
    markers=$(git diff --name-only --diff-filter=U | xargs grep -c '^<<<<<<<' 2>/dev/null | awk -F: '{s+=$2} END {print s+0}')
    printf "    conflict confined to: %s(%s marker(s))\n" "$files" "$markers"
    git merge --abort 2>/dev/null || true
  fi
}

echo "== cell-merge scenarios =="
run "S1 different cells" \
  'edit_entity level/cells/c_00_00.ron "$(uid 1005)" 50.0' \
  'edit_entity level/cells/c_00_01.ron "$(uid 2010)" 60.0' \
  clean

run "S2 same cell, different entities" \
  'edit_entity level/cells/c_00_00.ron "$(uid 1002)" 51.0' \
  'edit_entity level/cells/c_00_00.ron "$(uid 1015)" 61.0' \
  clean

run "S2b same cell, ADJACENT entities" \
  'edit_entity level/cells/c_00_00.ron "$(uid 1007)" 52.0' \
  'edit_entity level/cells/c_00_00.ron "$(uid 1008)" 62.0' \
  clean

run "S3 same cell, both add (far-apart uuids)" \
  'insert_entity level/cells/c_00_00.ron "$(uid 1003)-a"' \
  'insert_entity level/cells/c_00_00.ron "$(uid 1017)-b"' \
  clean

run "S3b same cell, both add (adjacent sort position)" \
  'insert_entity level/cells/c_00_00.ron "$(uid 1009)-a"' \
  'insert_entity level/cells/c_00_00.ron "$(uid 1009)-b"' \
  conflict

run "S4 same entity edited on both" \
  'edit_entity level/cells/c_00_00.ron "$(uid 1010)" 53.0' \
  'edit_entity level/cells/c_00_00.ron "$(uid 1010)" 63.0' \
  conflict

run "S5 delete vs edit same entity" \
  'delete_entity level/cells/c_00_00.ron "$(uid 1012)"' \
  'edit_entity level/cells/c_00_00.ron "$(uid 1012)" 64.0' \
  conflict

echo "== done =="
