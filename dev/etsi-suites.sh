# Sourced by dev/etsi-pipeline.sh and dev/etsi-run.sh — the ONE definition of
# which serial suites exist (was duplicated in both scripts).
SERIAL_ALL="CommonBehaviours ContextInformation/Consumption ContextInformation/Provision ContextInformation/Subscription ContextSource jsonldContext DistributedOperations"

# Completeness guard: a suite dir the runner doesn't know is a hard error, so
# a fork update can never add tests that silently don't run. Two roots:
# TP/NGSI-LD/* (serial suites) and repo-root *_TP trees (IOP_TP's pattern).
check_suites_complete() { # $1 = suite repo root
  local root="$1/TP/NGSI-LD" d s
  for d in "$root"/*/; do
    [ -d "$d" ] || continue
    d="$(basename "$d")"
    case " $SERIAL_ALL " in
      *" $d "*) ;;
      *" $d/"*) # listed only via subpaths — every immediate subdir must appear
        for s in "$root/$d"/*/; do
          [ -d "$s" ] || continue
          s="$(basename "$s")"
          case " $SERIAL_ALL " in *" $d/$s "*) ;; *)
            echo "TP/NGSI-LD/$d/$s is not in SERIAL_ALL (dev/etsi-suites.sh) — its tests would never run"
            return 1 ;;
          esac
        done ;;
      *)
        echo "TP/NGSI-LD/$d is not in SERIAL_ALL (dev/etsi-suites.sh) — its tests would never run"
        return 1 ;;
    esac
  done
  for d in "$1"/*_TP/; do
    [ -d "$d" ] || continue
    d="$(basename "$d")"
    case "$d" in IOP_TP) ;; *)
      echo "$d has no pipeline step (only IOP_TP is wired) — extend dev/etsi-pipeline.sh"
      return 1 ;;
    esac
  done
}
