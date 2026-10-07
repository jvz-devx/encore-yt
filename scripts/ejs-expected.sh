#!/usr/bin/env bash
# Solves a fixed set of JS challenges for a saved player script with yt-dlp's
# EJS solver in deno (what yt-dlp itself runs), for crates/core/tests/resolver_offline.rs.
#   scripts/ejs-expected.sh artifacts/resolver/<id>.js > artifacts/resolver/<id>.expected.json
# Offline: reads only the given file and the vendored EJS scripts.
set -euo pipefail
player=${1:?player script}
root="$(cd "$(dirname "$0")/.." && pwd)"
input="$(mktemp --suffix .js)"
trap 'rm -f "$input"' EXIT
python3 - "$player" "$root" > "$input" <<'PY'
import json, sys
player, root = sys.argv[1], sys.argv[2]
lib = open(f"{root}/crates/core/src/jsc/ejs-lib.min.js").read()
core = open(f"{root}/crates/core/src/jsc/ejs-core.min.js").read()
# Real challenges: the web SABR URL and WEB_CREATOR cipher of 2026-10-07.
n = ["_Kid_vJQOB_4Tt4Mq", "OREtw4lFskWoJnTXr", "ZdT9Xk3PqFwl0aB1c", "abcdefghijklmnop", "aAbBcCdDeEfFgG1234", "0123456789abcdefgh_-"]
lengths = list(range(90, 121))
data = {"type": "player", "player": open(player).read(), "output_preprocessed": False,
        "requests": [{"type": "n", "challenges": n},
                     {"type": "sig", "challenges": ["".join(map(chr, range(k))) for k in lengths]}]}
print(f"{lib}\nObject.assign(globalThis, lib);\n{core}\n"
      f"const out = jsc({json.dumps(data)});\n"
      "if (out.type !== 'result' || out.responses.some(r => r.type !== 'result')) { console.log(JSON.stringify(out)); Deno.exit(1); }\n"
      "const sig = {}; for (const [k, v] of Object.entries(out.responses[1].data)) sig[k.length] = [...v].map(c => c.charCodeAt(0));\n"
      "console.log(JSON.stringify({ n: out.responses[0].data, sig }));")
PY
deno run --quiet "$input"
