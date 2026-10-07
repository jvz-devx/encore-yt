#!/bin/bash
# measure LABEL: 10 s CPU % (of one core), PSS MB of the app (audio included), GPU render busy %.
label=$1
app=""; for p in /proc/[0-9]*; do e=$(readlink $p/exe 2>/dev/null); case "${e##*/}" in ytfast-gpui*) app=${p#/proc/};; esac; done
ticks() { local t=0; for p in "$@"; do [ -r /proc/$p/stat ] && t=$((t + $(awk '{print $14+$15}' /proc/$p/stat))); done; echo $t; }
pss() { local t=0; for p in "$@"; do [ -r /proc/$p/smaps_rollup ] && t=$((t + $(awk '/^Pss:/{print $2}' /proc/$p/smaps_rollup))); done; echo $((t/1024)); }
a0=$(ticks $app)
gpu=$(sudo timeout 11 intel_gpu_top -J -s 10000 -o - 2>/dev/null | python3 -c '
import sys,json,re
txt=sys.stdin.read().strip().rstrip(",")
txt="["+txt.lstrip("[").rstrip("]")+"]"
try:
  data=json.loads(re.sub(r",\s*\]","]",txt))
except Exception as e:
  print("n/a"); sys.exit()
s=[d for d in data if "engines" in d]
if not s: print("n/a"); sys.exit()
d=s[-1]["engines"]
r=[v["busy"] for k,v in d.items() if k.startswith("Render")]
print("%.1f" % r[0] if r else "n/a")')
a1=$(ticks $app)
hz=$(getconf CLK_TCK)
LC_NUMERIC=C printf "%-34s app cpu %5.1f%%  app pss %4d MB  gpu render %s%% (whole desktop)\n" "$label" \
  $(echo "($a1-$a0)/$hz*10" | bc -l) $(pss $app) "$gpu"
