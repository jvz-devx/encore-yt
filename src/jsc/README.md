This folder only keeps `pins.txt` at the path ytfast-gpui 0.1.x reads it
from (raw.githubusercontent.com, `main`), so those installs still get new
solver pins. The real file is `crates/core/src/jsc/pins.txt`;
`scripts/ejs-bump.sh` writes both. Remove this folder once no 0.1.x
installs remain.
